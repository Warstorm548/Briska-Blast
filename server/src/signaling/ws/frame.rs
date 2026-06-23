//! Phase-3 inbound routing: parse a single client text frame and dispatch it to
//! the [`SignalHub`](crate::signaling::SignalHub) (relay an SDP/ICE message,
//! record a score, answer a time-sync, …). Returns whether the pump loop in
//! [`super`] should keep running.

use chrono::Utc;

use crate::{
    api::fetch_usernames,
    signaling::protocol::{ClientMsg, ServerMsg},
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
            // Symmetric-NAT kick logic is deferred to a follow-up branch
            // (TURN relay work). For now, just log.
            tracing::info!(
                "ws: {} reports peer_connection_failed against {} (reason: {})",
                from_player, peer, reason
            );
        }
        ClientMsg::ReportScore { scoring_player_id } => {
            // Trusted for now: any member may report, and the reported
            // scorer is taken at face value. Server-side trajectory
            // validation is the documented later hook. Broadcast to
            // everyone (including the reporter) so all clients converge on
            // the server's authoritative tally rather than a local guess.
            //
            // CodeRabbit (PR #78) flagged that the win condition now lets a
            // forged report *end* the match, not just pad the score. Deferred:
            // this mode has the scored-on player report a *different* scorer, so
            // there's no minimal authz check — the fix is the trajectory
            // validation tracked in docs/planning/roadmap.md ("Server-side
            // validation of score reports").
            if let Some((scores, winner)) =
                state.signal_hub.record_score(code, &scoring_player_id).await
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

            // Resolve the sender's display name fresh from Redis so a mid-session
            // rename is reflected; a miss or Redis error degrades to empty (the
            // client then shows `Player <id>`), never dropping the message.
            let username = match state.redis.get().await {
                Ok(mut conn) => {
                    let ids = [from_player.to_string()];
                    fetch_usernames(&mut conn, &ids)
                        .await
                        .remove(from_player)
                        .unwrap_or_default()
                }
                Err(_) => String::new(),
            };

            // Broadcast to everyone including the sender so all clients render an
            // identical, server-ordered transcript (same rationale as ScoreUpdate).
            state
                .signal_hub
                .broadcast(
                    code,
                    ServerMsg::ChatMessage {
                        from: from_player.to_string(),
                        username,
                        text,
                    },
                    None,
                )
                .await;
        }
        ClientMsg::Leave => return false,
    }
    true
}
