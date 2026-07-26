//! Phase-3 inbound routing: parse a single client text frame and dispatch it to
//! the [`SignalHub`](crate::signaling::SignalHub) (relay an SDP/ICE message,
//! record a score, answer a time-sync, …). Returns whether the pump loop in
//! [`super`] should keep running.

use chrono::Utc;

use crate::{
    chat::blacklist,
    signaling::protocol::{ChatKind, ClientMsg, ServerMsg},
    signaling::ReadyOutcome,
    state::AppState,
};

/// Upper bound on a single chat message after trimming. Keeps one client from
/// flooding the room with an oversized frame; the client also limits input.
const MAX_CHAT_LEN: usize = 500;

/// Parse and route a single incoming text frame. Returns true to keep
/// the loop running, false on explicit Leave.
pub(super) async fn handle_client_frame(
    text: &str,
    code: &str,
    from_player: &str,
    state: &AppState,
) -> bool {
    let msg: ClientMsg = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("ws: malformed frame from {}: {}", from_player, e);
            return true; // ignore and continue
        }
    };

    match msg {
        ClientMsg::Identify { .. } => {
            // Duplicate identify on an already-identified WS. Policy is
            // documented in the edge-cases doc; current behavior is
            // ignore. Will become kick-old in a follow-up.
            tracing::debug!("ws: duplicate identify from {} — ignored", from_player);
        }
        ClientMsg::Offer { to, sdp } => {
            tracing::debug!(%to, "relay offer");
            state
                .signal_hub
                .send_to(
                    code,
                    &to,
                    ServerMsg::Offer { from: from_player.to_string(), sdp },
                )
                .await;
        }
        ClientMsg::Answer { to, sdp } => {
            tracing::debug!(%to, "relay answer");
            state
                .signal_hub
                .send_to(
                    code,
                    &to,
                    ServerMsg::Answer { from: from_player.to_string(), sdp },
                )
                .await;
        }
        ClientMsg::IceCandidate { to, candidate, sdp_mid, sdp_m_line_index } => {
            // Candidate type (host/srflx/relay) makes NAT-traversal progress
            // visible server-side — a run that never yields a connectable pair is
            // the symmetric-NAT / no-TURN case.
            tracing::debug!(%to, cand_type = candidate_type(&candidate), "relay ice");
            state
                .signal_hub
                .send_to(
                    code,
                    &to,
                    ServerMsg::IceCandidate {
                        from: from_player.to_string(),
                        candidate,
                        sdp_mid,
                        sdp_m_line_index,
                    },
                )
                .await;
        }
        ClientMsg::PeerConnectionFailed { peer, reason } => {
            // Symmetric-NAT kick logic is deferred to a follow-up branch (TURN
            // relay work). For now, log it as a warning with structured fields — a
            // peer pair that can't connect is a real connectivity problem (usually
            // symmetric NAT with no TURN relay yet). `session`/`player` come from
            // the connection span.
            tracing::warn!(%peer, %reason, "peer connection failed");
        }
        ClientMsg::ReportScore { scoring_player_id, points } => {
            // Trusted for now: any member may report, and the reported
            // scorer is taken at face value. Server-side trajectory
            // validation is the documented later hook. Broadcast to
            // everyone (including the reporter) so all clients converge on
            // the server's authoritative tally rather than a local guess.
            //
            // CodeRabbit (PR #78) flagged that the win condition now lets a
            // forged report *end* the match, not just pad the score. The `points`
            // value is likewise client-supplied; the server can't derive it without
            // tracking ball state (the sim is client-authoritative / P2P), so it only
            // bounds it (`record_score` rejects <= 0 and clamps to the max). Deferred:
            // this mode has the scored-on player report a *different* scorer, so
            // there's no minimal authz check — the real fix is the trajectory
            // validation tracked in docs/planning/roadmap.md ("Server-side
            // validation of score reports").
            if let Some((scores, winner)) =
                state.signal_hub.record_score(code, &scoring_player_id, points).await
            {
                state
                    .signal_hub
                    .broadcast(code, ServerMsg::ScoreUpdate { scores: scores.clone() }, None)
                    .await;

                // Win condition met. GameOver is a pure UI signal (freeze + show
                // the leaderboard); send it FIRST so every client latches its
                // game-over state, THEN hand the actual session teardown to the
                // shared SessionEnded path. Same per-client ordered channel, so
                // GameOver always lands before the SessionEnded auto-leave (which
                // the client suppresses once it's in game-over).
                if let Some(winner_player_id) = winner {
                    state
                        .signal_hub
                        .broadcast(code, ServerMsg::GameOver { winner_player_id, scores }, None)
                        .await;
                    super::session_ops::end_session(state, code, "game_over").await;
                }
            }
        }
        ClientMsg::TimeSync { client_send_ms } => {
            // Reply on this same connection with the server's clock. The client
            // echoes its own send time so we stay stateless; it derives the
            // offset (see `ServerClock`). `Utc::now()` is the shared reference
            // every peer syncs to so handoff timestamps are comparable.
            state
                .signal_hub
                .send_to(
                    code,
                    from_player,
                    ServerMsg::TimeSync {
                        client_send_ms,
                        server_ms: Utc::now().timestamp_millis(),
                    },
                )
                .await;
        }
        ClientMsg::SendChat { text } => {
            // Drop empties (a stray Enter shouldn't broadcast a blank line) and
            // bound the length. Truncate on a char boundary so multi-byte input
            // can't panic.
            let text = text.trim();
            if text.is_empty() {
                return true;
            }
            let text: String = text.chars().take(MAX_CHAT_LEN).collect();

            // Censor before broadcast, not after: players are sent the masked
            // form, so the raw word never leaves the server and no client-side
            // support is needed. The uncensored original goes to the moderation
            // transcript only. Matching reads a cached word list, so this still
            // works during a Redis outage.
            let words = blacklist::active_words(state).await;
            let matches = blacklist::find_matches(&text, &words);
            let flagged_words = blacklist::matched_words(&matches);
            let broadcast_text = blacklist::mask(&text, &matches);

            // Capture assigns the body id, stores the line, and resolves the
            // display name. It degrades to an empty name (client shows
            // `Player <id>`) rather than dropping the message — moderation
            // failing must never silence chat.
            let username = crate::chat::capture_player_message(
                state,
                code,
                from_player,
                &text,
                flagged_words,
            )
            .await;

            // Broadcast to everyone including the sender so all clients render an
            // identical, server-ordered transcript (same rationale as ScoreUpdate).
            state
                .signal_hub
                .broadcast(
                    code,
                    ServerMsg::ChatMessage {
                        from: from_player.to_string(),
                        username,
                        text: broadcast_text,
                        kind: ChatKind::Player,
                    },
                    None,
                )
                .await;
        }
        ClientMsg::ClientReady => {
            // The ready barrier (template: ReportScore — thin arm, testable hub
            // method). The hub's `match_started` latch guarantees AllReady is
            // handed to exactly one caller, so the broadcast + activation can't
            // double-fire against the grace valve.
            match state.signal_hub.record_ready(code, from_player).await {
                ReadyOutcome::AllReady => {
                    tracing::info!("ws: all players ready — starting match");
                    super::session_ops::start_match(state, code).await;
                }
                ReadyOutcome::AlreadyStarted => {
                    // Straggler (barrier timed out, a poll-fallback recovery, or
                    // a mid-match rejoiner): converge it with a direct reply
                    // rather than leaving it waiting for a broadcast that
                    // already happened. A rejoiner's ready also releases its
                    // pause hold — resume the room BEFORE the go-signal so the
                    // countdown is already running when the rejoiner lands.
                    super::session_ops::resume_if_cleared(state, code, from_player).await;
                    state
                        .signal_hub
                        .send_to(code, from_player, ServerMsg::MatchStarted {})
                        .await;
                }
                ReadyOutcome::Pending => {
                    tracing::debug!("ws: client ready — waiting on others");
                }
                ReadyOutcome::NotSeated => {
                    tracing::debug!("ws: client_ready from unseated player — ignored");
                }
            }
        }
        ClientMsg::Leave => return false,
    }
    true
}

/// Extract the ICE candidate type (host / srflx / relay / prflx) from a candidate
/// SDP line, for NAT diagnosis in the relay trace. Returns `"end"` for the empty
/// end-of-candidates sentinel and `"unknown"` when no `typ` token is present.
fn candidate_type(candidate: &str) -> &str {
    if candidate.is_empty() {
        return "end";
    }
    match candidate.split_once("typ ") {
        Some((_, rest)) => rest.split(' ').next().unwrap_or("unknown"),
        None => "unknown",
    }
}
