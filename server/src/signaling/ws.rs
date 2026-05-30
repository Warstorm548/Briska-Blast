use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, State,
    },
    response::IntoResponse,
};
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::time::Duration;

use crate::{
    api::{validate_player, Session},
    signaling::{
        protocol::{ClientMsg, ServerMsg},
        GraceKind,
    },
    state::AppState,
};
use shared::types::session::SessionStatus;

// App-defined close codes in the WebSocket 4xxx range. Chosen to mirror
// the HTTP semantics a launcher developer already knows from REST.
const CLOSE_BAD_INITIAL: u16 = 4400;
const CLOSE_UNAUTHORIZED: u16 = 4401;
const CLOSE_FORBIDDEN: u16 = 4403;
const CLOSE_NOT_FOUND: u16 = 4404;
const CLOSE_INTERNAL: u16 = 4500;

// The first frame after upgrade must arrive within this window or the
// server closes the connection. Prevents a malicious or buggy client
// from pinning a connection that never identifies.
const IDENTIFY_DEADLINE: Duration = Duration::from_secs(5);

/// How long a mid-game host has to re-Identify after their WebSocket drops
/// before the server promotes the next player in join order (or ends the
/// session). Also the duration peers show a "reconnecting…" overlay (broadcast
/// in `HostReconnecting` / `PeerReconnecting`). Const for now; promoting it to
/// runtime config is a later refinement.
const PROMOTION_GRACE: Duration = Duration::from_secs(30);

/// How long ANY dropped mid-game player's session slot is held so they can
/// rejoin by re-entering the code, before it is freed permanently. Longer than
/// `PROMOTION_GRACE` so a full process relaunch + manual code entry can make it
/// back. A promoted-away ex-host is demoted into `joiners` (kept, not removed)
/// and keeps the remainder of this window, rejoining as a non-host.
const RECONNECT_GRACE: Duration = Duration::from_secs(120);

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(code): Path<String>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, code, state))
}

async fn handle_socket(mut socket: WebSocket, code: String, state: AppState) {
    // Phase 1: Identify-frame auth with deadline.
    let (player_id, is_host, host_player_id) = match identify(&mut socket, &code, &state).await {
        Ok(triple) => triple,
        Err(_) => return, // identify() already closed the socket with the right code
    };

    // Phase 2: Register with SignalHub and announce arrival.
    // conn_id scopes the eventual leave_room call so an older socket's
    // cleanup can't evict a newer socket that re-bound this player_id.
    let (conn_id, mut rx) = state.signal_hub.join_room(&code, &player_id).await;

    // Snapshot peers at identify time so the client doesn't need a poll.
    let peers = match peer_roster(&state, &code, &player_id).await {
        Ok(p) => p,
        Err(_) => {
            // Session must have been deleted between membership check
            // and now — vanishingly rare race, but bail cleanly.
            close_with(&mut socket, CLOSE_NOT_FOUND, "session_gone").await;
            state.signal_hub.leave_room(&code, &player_id, conn_id).await;
            return;
        }
    };

    let identified = ServerMsg::Identified {
        your_player_id: player_id.clone(),
        host_player_id,
        peers,
        is_host,
    };
    if let Ok(text) = serde_json::to_string(&identified) {
        let _ = socket.send(Message::Text(text)).await;
    }

    state
        .signal_hub
        .broadcast(
            &code,
            ServerMsg::PeerJoined { player_id: player_id.clone() },
            Some(&player_id),
        )
        .await;

    tracing::info!(
        "ws: player {} identified in session {} (is_host={})",
        player_id, code, is_host
    );

    // Back from a mid-game drop: cancel the slot-hold timer so it doesn't free
    // our slot now that we're present — host or joiner alike. No-op when none
    // is armed (a fresh first identify, or already reconnected).
    state
        .signal_hub
        .take_grace(&code, &player_id, GraceKind::Reconnect)
        .await;

    // A host that returns *before* promotion also cancels the promotion timer
    // (take_grace wins the race against the timer firing) and tells peers it's
    // back so they clear the "host reconnecting…" state. After promotion the
    // ex-host re-Identifies as a normal joiner (is_host == false, the Promotion
    // entry already claimed by the timer), so this is skipped and they simply
    // re-mesh via the PeerJoined broadcast above.
    if is_host
        && state
            .signal_hub
            .take_grace(&code, &player_id, GraceKind::Promotion)
            .await
    {
        state
            .signal_hub
            .broadcast(
                &code,
                ServerMsg::HostReconnected { player_id: player_id.clone() },
                None,
            )
            .await;
        tracing::info!("ws: host {} reconnected to session {} within grace", player_id, code);
    }

    // Phase 3: Pump messages in both directions until either side closes.
    // Single owner of the WS — no split, no spawned tasks, no abort
    // coordination. select! polls both arms; either completion drops us
    // into cleanup.
    //
    // `explicit_leave` records that the client sent a `Leave` frame (a
    // deliberate "I'm out") rather than the socket merely dropping. Only a
    // deliberate leave frees a joiner's Redis slot — a transient drop keeps
    // the slot so the player can reconnect and re-Identify.
    let mut explicit_leave = false;
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if !handle_client_frame(&text, &code, &player_id, &state).await {
                            explicit_leave = true;
                            break; // explicit Leave
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        // Axum auto-pongs by default, but be explicit for clarity.
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(_)) => {
                        // Binary/Pong frames — ignore.
                    }
                    Some(Err(e)) => {
                        tracing::debug!("ws: recv error from {}: {}", player_id, e);
                        break;
                    }
                    None => break, // stream ended
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Some(msg) => {
                        let text = match serde_json::to_string(&msg) {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::error!("ws: failed to serialize ServerMsg: {}", e);
                                continue;
                            }
                        };
                        if socket.send(Message::Text(text)).await.is_err() {
                            break; // peer hung up
                        }
                    }
                    None => break, // hub dropped the sender
                }
            }
        }
    }

    // Phase 4: Cleanup. leave_room is scoped to our conn_id so a newer
    // socket that re-bound this player_id (duplicate identify) isn't
    // evicted by our late cleanup.
    state.signal_hub.leave_room(&code, &player_id, conn_id).await;

    if is_host {
        // The host always announces departure (peers update their roster /
        // overlay); past Waiting it also arms grace or promotes.
        broadcast_peer_left(&state, &code, &player_id, explicit_leave).await;

        // Waiting: tear the lobby down (joiners can't start without a host).
        // Past Waiting (a live match): the session must survive host loss.
        match end_session_if_waiting(&state, &code).await {
            Ok(HostDisconnectStage::EndedWaiting) | Ok(HostDisconnectStage::SessionGone) => {}
            Ok(HostDisconnectStage::Active) => {
                if explicit_leave {
                    // Deliberate mid-game quit: promote now and DROP the ex-host
                    // (keep=false) — they left, they're not coming back.
                    if let Err(e) =
                        promote_demote_or_end_active(&state, &code, &player_id, false).await
                    {
                        tracing::warn!("ws: host-leave promotion failed for {}: {}", code, e);
                    }
                } else {
                    // Transient drop: arm the 30s promotion timer AND the 2-min
                    // reconnect slot-hold. If promotion fires, the ex-host is
                    // demoted into joiners (kept) so they keep the rest of their
                    // window and rejoin as a non-host.
                    arm_host_disconnect_grace(&state, &code, &player_id).await;
                }
            }
            Err(e) => tracing::warn!("ws: host-disconnect cleanup failed for {}: {}", code, e),
        }
    } else if explicit_leave {
        // A joiner deliberately left. Announce, then free their Redis slot so the
        // roster stays honest: in Waiting this keeps lobby capacity / `/start`
        // correct; past Waiting it keeps GET /session accurate and ends the
        // session if the host is now alone.
        broadcast_peer_left(&state, &code, &player_id, true).await;
        if let Err(e) = remove_joiner_on_leave(&state, &code, &player_id).await {
            tracing::warn!("ws: joiner-leave cleanup failed for {} in {}: {}", player_id, code, e);
        }
    } else if session_status_is_active(&state, &code).await {
        // Joiner transient drop mid-game: hold the slot for a rejoin window and
        // show peers a "reconnecting…" overlay. The final PeerLeft is deferred
        // to the slot-hold timeout (or superseded by PeerJoined on rejoin) —
        // do NOT announce a leave now.
        arm_joiner_disconnect_grace(&state, &code, &player_id).await;
    } else {
        // Joiner transient drop in Waiting (or the session is already gone):
        // legacy behavior — announce, keep the slot for an in-lobby reconnect.
        broadcast_peer_left(&state, &code, &player_id, false).await;
    }

    tracing::info!("ws: player {} disconnected from session {}", player_id, code);
}

/// Broadcast a `PeerLeft` with the right reason for a deliberate vs transient
/// departure (the common shape used across the cleanup branches).
async fn broadcast_peer_left(state: &AppState, code: &str, player_id: &str, explicit_leave: bool) {
    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::PeerLeft {
                player_id: player_id.to_string(),
                reason: if explicit_leave { "leave" } else { "disconnect" },
            },
            None,
        )
        .await;
}

/// Read-only: is `code` a live match (past Waiting)? A missing / undecodable
/// session, or one that's `Ended`, counts as not active.
async fn session_status_is_active(state: &AppState, code: &str) -> bool {
    let Ok(mut conn) = state.redis.get().await else {
        return false;
    };
    let raw: Option<String> = conn.get(format!("session:{}", code)).await.unwrap_or(None);
    let Some(raw) = raw else {
        return false;
    };
    matches!(
        serde_json::from_str::<Session>(&raw).map(|s| s.status),
        Ok(SessionStatus::Starting) | Ok(SessionStatus::Active)
    )
}

/// Arm a dropped mid-game host's grace: a 30s `Promotion` timer (promote the
/// next player, demoting this host into joiners so they keep their window) AND
/// the shared 2-min `Reconnect` slot-hold (below). Announces `HostReconnecting`
/// so peers show the overlay. The reconnect path takes whichever entry applies,
/// waking the relevant timer task early.
async fn arm_host_disconnect_grace(state: &AppState, code: &str, host_id: &str) {
    let Some(promo_rx) = state.signal_hub.arm_grace(code, host_id, GraceKind::Promotion).await
    else {
        // The host already has a live socket (it reconnected on a new connection
        // before this stale disconnect path ran) — don't arm or announce grace
        // against a host that's actually present.
        tracing::info!("ws: host {} already present in session {}, grace not armed", host_id, code);
        return;
    };

    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::HostReconnecting {
                player_id: host_id.to_string(),
                grace_secs: PROMOTION_GRACE.as_secs(),
            },
            None,
        )
        .await;
    tracing::info!("ws: host {} dropped from active session {} — {}s promotion grace, {}s rejoin hold",
        host_id, code, PROMOTION_GRACE.as_secs(), RECONNECT_GRACE.as_secs());

    // Promotion timer: at PROMOTION_GRACE, claim the entry (single-winner vs the
    // host reconnecting) and promote the next player, keeping the ex-host as a
    // demoted joiner so they can still rejoin within the reconnect hold.
    {
        let state = state.clone();
        let code = code.to_string();
        let host_id = host_id.to_string();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(PROMOTION_GRACE) => {
                    if state.signal_hub.take_grace(&code, &host_id, GraceKind::Promotion).await {
                        if let Err(e) = promote_demote_or_end_active(&state, &code, &host_id, true).await {
                            tracing::warn!("ws: host-grace promotion failed for {}: {}", code, e);
                        }
                    }
                }
                _ = promo_rx => { /* host re-Identified before promotion */ }
            }
        });
    }

    // Reconnect slot-hold for the (possibly demoted) ex-host.
    arm_reconnect_slot_hold(state, code, host_id.to_string()).await;
}

/// Arm a dropped mid-game joiner's reconnect window: announce `PeerReconnecting`
/// (peers show the overlay) and start the shared 2-min slot-hold. No promotion
/// timer — joiners aren't host candidates.
async fn arm_joiner_disconnect_grace(state: &AppState, code: &str, player_id: &str) {
    // Arm the hold + spawn the timer; only announce the overlay when a hold was
    // actually armed (false ⇒ the joiner already reconnected on a new socket).
    if arm_reconnect_slot_hold(state, code, player_id.to_string()).await {
        state
            .signal_hub
            .broadcast(
                code,
                ServerMsg::PeerReconnecting {
                    player_id: player_id.to_string(),
                    grace_secs: PROMOTION_GRACE.as_secs(),
                },
                None,
            )
            .await;
        tracing::info!("ws: joiner {} dropped from active session {} — {}s rejoin hold",
            player_id, code, RECONNECT_GRACE.as_secs());
    }
}

/// Arm (if not already) the 2-min `Reconnect` slot-hold for `player_id` and spawn
/// the timer that frees their slot when it elapses. Returns `true` if a hold is
/// now pending (false if the player already has a live socket, so nothing to
/// hold). Shared by the host and joiner disconnect paths.
async fn arm_reconnect_slot_hold(state: &AppState, code: &str, player_id: String) -> bool {
    let Some(recon_rx) = state
        .signal_hub
        .arm_grace(code, &player_id, GraceKind::Reconnect)
        .await
    else {
        return false;
    };

    let state = state.clone();
    let code = code.to_string();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(RECONNECT_GRACE) => {
                // Window elapsed: claim it (single-winner vs a late reconnect)
                // and free the slot for good.
                if state.signal_hub.take_grace(&code, &player_id, GraceKind::Reconnect).await {
                    free_member_after_timeout(&state, &code, &player_id).await;
                }
            }
            _ = recon_rx => { /* player rejoined within the window */ }
        }
    });
    true
}

/// A held slot's rejoin window elapsed: tell peers the player is gone for good
/// and free their Redis slot (reusing the explicit-leave removal, which also
/// ends the session if this leaves the host alone).
async fn free_member_after_timeout(state: &AppState, code: &str, player_id: &str) {
    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::PeerLeft { player_id: player_id.to_string(), reason: "reconnect_timeout" },
            None,
        )
        .await;
    if let Err(e) = remove_joiner_on_leave(state, code, player_id).await {
        tracing::warn!("ws: reconnect-timeout cleanup failed for {} in {}: {}", player_id, code, e);
    }
    tracing::info!("ws: player {} rejoin window elapsed in session {} — slot freed", player_id, code);
}

/// Reads the first frame, validates the token, confirms membership.
/// Closes the socket with the appropriate 4xxx code on any failure.
/// On success returns `(player_id, is_host, host_player_id)`.
async fn identify(
    socket: &mut WebSocket,
    code: &str,
    state: &AppState,
) -> Result<(String, bool, String), ()> {
    let first = match tokio::time::timeout(IDENTIFY_DEADLINE, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            close_with(socket, CLOSE_BAD_INITIAL, "identify_required").await;
            return Err(());
        }
    };

    let (player_id, secret_token) = match serde_json::from_str::<ClientMsg>(&first) {
        Ok(ClientMsg::Identify { player_id, secret_token }) => (player_id, secret_token),
        _ => {
            close_with(socket, CLOSE_BAD_INITIAL, "identify_required").await;
            return Err(());
        }
    };

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => {
            close_with(socket, CLOSE_INTERNAL, "redis_unavailable").await;
            return Err(());
        }
    };

    if validate_player(&mut conn, &player_id, &secret_token).await.is_err() {
        close_with(socket, CLOSE_UNAUTHORIZED, "unauthorized").await;
        return Err(());
    }

    let raw: Option<String> = match conn.get(format!("session:{}", code)).await {
        Ok(r) => r,
        Err(_) => {
            close_with(socket, CLOSE_INTERNAL, "redis_get_failed").await;
            return Err(());
        }
    };
    let raw = match raw {
        Some(r) => r,
        None => {
            close_with(socket, CLOSE_NOT_FOUND, "session_not_found").await;
            return Err(());
        }
    };
    let session: Session = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(_) => {
            close_with(socket, CLOSE_INTERNAL, "session_decode_failed").await;
            return Err(());
        }
    };

    if !session.contains_player(&player_id) {
        close_with(socket, CLOSE_FORBIDDEN, "not_in_session").await;
        return Err(());
    }

    let is_host = session.host_player_id == player_id;
    Ok((player_id, is_host, session.host_player_id))
}

/// Roster of everyone in the session EXCEPT the requesting player. Used
/// in the Identified frame so the client doesn't need a separate poll.
async fn peer_roster(
    state: &AppState,
    code: &str,
    self_player_id: &str,
) -> Result<Vec<String>, ()> {
    let mut conn = state.redis.get().await.map_err(|_| ())?;
    let raw: Option<String> = conn.get(format!("session:{}", code)).await.map_err(|_| ())?;
    let raw = raw.ok_or(())?;
    let session: Session = serde_json::from_str(&raw).map_err(|_| ())?;

    let peers = std::iter::once(session.host_player_id)
        .chain(session.joiners.into_iter().map(|j| j.player_id))
        .filter(|p| p != self_player_id)
        .collect();
    Ok(peers)
}

/// Parse and route a single incoming text frame. Returns true to keep
/// the loop running, false on explicit Leave.
async fn handle_client_frame(
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
            if let Some(scores) = state.signal_hub.record_score(code, &scoring_player_id).await {
                state
                    .signal_hub
                    .broadcast(code, ServerMsg::ScoreUpdate { scores }, None)
                    .await;
            }
        }
        ClientMsg::Leave => return false,
    }
    true
}

/// What the host's disconnect cleanup found, so the caller knows whether to
/// promote. `end_session_if_waiting` both *acts* (ends the lobby if Waiting)
/// and *classifies* (reports when the match is live and needs promotion).
enum HostDisconnectStage {
    /// Session was still Waiting; it has been deleted and `SessionEnded`
    /// broadcast. Nothing more for the caller to do.
    EndedWaiting,
    /// Session no longer exists (already gone). Nothing to do.
    SessionGone,
    /// Session is past Waiting (a live match). The caller promotes the next
    /// player or arms the reconnect grace window.
    Active,
}

/// On host disconnect: if the session is still in Waiting, end it and notify
/// peers (joiners can't start without a host) and return `EndedWaiting`. Past
/// Waiting, leave the session intact and return `Active` so the caller can
/// promote / arm grace.
///
/// Atomicity matters here: a non-atomic GET-then-DEL races with
/// `/session/:code/start` transitioning Waiting → Starting between the
/// two calls. A single Lua script performs the read, status check, and
/// conditional DEL as one Redis operation — no other command can
/// interleave.
async fn end_session_if_waiting(state: &AppState, code: &str) -> Result<HostDisconnectStage, String> {
    const END_IF_WAITING_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_waiting'})
end
redis.call('DEL', KEYS[1])
return cjson.encode({result = 'deleted'})
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum EndOutcome {
        Deleted,
        NotFound,
        NotWaiting,
    }

    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(END_IF_WAITING_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: EndOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed lua result: {e}"))?;

    match outcome {
        EndOutcome::NotFound => Ok(HostDisconnectStage::SessionGone),
        // Advanced past Waiting (likely a concurrent /start): the match is live.
        EndOutcome::NotWaiting => Ok(HostDisconnectStage::Active),
        EndOutcome::Deleted => {
            state
                .signal_hub
                .broadcast(
                    code,
                    ServerMsg::SessionEnded { reason: "host_disconnect" },
                    None,
                )
                .await;
            tracing::info!("ws: session {} ended (host disconnected during waiting)", code);
            Ok(HostDisconnectStage::EndedWaiting)
        }
    }
}

/// Promote the next player after a host is lost from a live (past-Waiting)
/// session, or end the session if too few players remain to continue. Picks the
/// **oldest still-connected joiner** — chronological join order, skipping any
/// joiner whose WS is gone — and requires at least two connected players to
/// keep playing (a lone survivor has no one to play, matching the design's
/// "1 player remains → session ends"). Atomic single Lua script, guarded on the
/// departing player still being the host so a double disconnect can't promote
/// twice.
///
/// When `keep_ex_host` is true (a transient host drop), the departing host is
/// **demoted into `joiners`** at the back of the join order so they remain a
/// member and can rejoin as a non-host within their reconnect window. When false
/// (a deliberate host `Leave`), they're dropped. In the promoted branch
/// `joiners` is always non-empty (promotion needs ≥2 connected joiners, so ≥1
/// remains after removing the promoted one), so the empty-table re-encode quirk
/// can't arise.
async fn promote_demote_or_end_active(
    state: &AppState,
    code: &str,
    departing_host: &str,
    keep_ex_host: bool,
) -> Result<(), String> {
    // KEYS[1] = session key
    // ARGV[1] = departing host player_id (guard)
    // ARGV[2] = JSON array of currently-connected player_ids
    // ARGV[3] = session TTL seconds
    // ARGV[4] = now in ms (joined_at_ms for the demoted ex-host)
    // ARGV[5] = "1" to demote-and-keep the ex-host, "0" to drop them
    const PROMOTE_OR_END_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'gone'})
end
local session = cjson.decode(raw)
if session.status == 'waiting' then
  return cjson.encode({result = 'not_active'})
end
if session.host_player_id ~= ARGV[1] then
  return cjson.encode({result = 'not_host'})
end
local connected = {}
for _, pid in ipairs(cjson.decode(ARGV[2])) do
  connected[pid] = true
end
local promote_idx = nil
local connected_count = 0
for i, j in ipairs(session.joiners) do
  if connected[j.player_id] then
    connected_count = connected_count + 1
    if promote_idx == nil then
      promote_idx = i
    end
  end
end
if connected_count >= 2 then
  local new_host = session.joiners[promote_idx].player_id
  table.remove(session.joiners, promote_idx)
  session.host_player_id = new_host
  if ARGV[5] == '1' then
    -- Demote the ex-host to the back of the join order (kept for reconnect).
    table.insert(session.joiners, {player_id = ARGV[1], joined_at_ms = tonumber(ARGV[4])})
  end
  redis.call('SET', KEYS[1], cjson.encode(session), 'EX', tonumber(ARGV[3]))
  return cjson.encode({result = 'promoted', new_host = new_host})
else
  redis.call('DEL', KEYS[1])
  return cjson.encode({result = 'ended'})
end
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum PromoteOutcome {
        Promoted { new_host: String },
        Ended,
        Gone,
        NotActive,
        NotHost,
    }

    // Live members at promotion time. The departing host's sender was already
    // removed by leave_room before this runs, so it won't appear here; filter
    // anyway to be defensive.
    let connected: Vec<String> = state
        .signal_hub
        .room_members(code)
        .await
        .into_iter()
        .filter(|p| p != departing_host)
        .collect();
    let connected_json = serde_json::to_string(&connected).map_err(|e| e.to_string())?;

    let now_ms = Utc::now().timestamp_millis().max(0) as u64;
    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(PROMOTE_OR_END_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .arg(departing_host)
        .arg(connected_json)
        .arg(state.config.session_ttl_secs)
        .arg(now_ms)
        .arg(if keep_ex_host { "1" } else { "0" })
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: PromoteOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed promote lua result: {e}"))?;

    match outcome {
        PromoteOutcome::Promoted { new_host } => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::HostChanged { player_id: new_host.clone() }, None)
                .await;
            tracing::info!("ws: session {} promoted {} to host after host loss", code, new_host);
        }
        PromoteOutcome::Ended => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::SessionEnded { reason: "host_disconnect" }, None)
                .await;
            tracing::info!("ws: session {} ended after host loss (too few players remain)", code);
        }
        // Already handled by another path (concurrent promotion, the session
        // was torn down elsewhere, or it was never active). No broadcast.
        PromoteOutcome::Gone | PromoteOutcome::NotActive | PromoteOutcome::NotHost => {
            tracing::debug!("ws: promote_demote_or_end_active for {} was a no-op", code);
        }
    }
    Ok(())
}

/// On an explicit joiner leave, remove that joiner from the session's Redis
/// roster. In Waiting this frees a lobby slot (so capacity / `/start`'s
/// all-peers-ready check stay correct); past Waiting it keeps GET /session
/// honest and, if the leave empties the roster (only the host remains), ends
/// the now-unplayable match. A transient socket drop never reaches here — it
/// keeps the slot for reconnect.
///
/// Atomic for the same reason `end_session_if_waiting` is: a non-atomic
/// GET-then-SET races with `/session/:code/start`. Here the single Lua
/// script reads, removes, status-checks, and writes/deletes as one operation.
/// `/start`'s own CAS (its `expected_joiner_count` guard) handles the
/// cross-operation race — if this removal lands between /start's WS-ready
/// check and its transaction, /start sees the count mismatch and retries.
async fn remove_joiner_on_leave(
    state: &AppState,
    code: &str,
    player_id: &str,
) -> Result<(), String> {
    // lua-cjson encodes an empty Lua table as `{}` (a JSON object). If
    // removing the last joiner empties `session.joiners`, the naive re-encode
    // would write `"joiners":{}`, which then fails to deserialize into the
    // Rust `Vec<JoinerEntry>`. The `string.gsub` below forces it back to `[]`
    // in that one case.
    //
    // Safety of the literal-substring gsub: it intentionally targets the exact
    // JSON fragment lua-cjson produces for an empty `session.joiners`. No other
    // session field can contain that substring — `code` is from a brace-free
    // alphabet, `host_player_id` is digits, `gamemode`/`status` are fixed
    // lowercase words, and `player_count` is a number. Revisit this gsub if the
    // Session shape gains a free-form string field.
    const REMOVE_JOINER_ON_LEAVE_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
local found_idx = nil
for i, j in ipairs(session.joiners) do
  if j.player_id == ARGV[1] then
    found_idx = i
    break
  end
end
if not found_idx then
  return cjson.encode({result = 'not_joiner'})
end
table.remove(session.joiners, found_idx)
if session.status ~= 'waiting' and #session.joiners == 0 then
  redis.call('DEL', KEYS[1])
  return cjson.encode({result = 'ended'})
end
local encoded = cjson.encode(session)
if #session.joiners == 0 then
  encoded = string.gsub(encoded, '"joiners":{}', '"joiners":[]')
end
redis.call('SET', KEYS[1], encoded, 'EX', tonumber(ARGV[2]))
return cjson.encode({result = 'removed'})
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum RemoveOutcome {
        Removed,
        Ended,
        NotFound,
        NotJoiner,
    }

    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(REMOVE_JOINER_ON_LEAVE_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .arg(player_id)
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: RemoveOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed lua result: {e}"))?;

    match outcome {
        RemoveOutcome::Removed => {
            tracing::info!("ws: removed joiner {} from session {}", player_id, code);
        }
        RemoveOutcome::Ended => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::SessionEnded { reason: "last_player_left" }, None)
                .await;
            tracing::info!("ws: session {} ended (last peer left, host alone)", code);
        }
        RemoveOutcome::NotFound | RemoveOutcome::NotJoiner => {}
    }
    Ok(())
}

async fn close_with(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: Cow::Borrowed(reason),
    };
    let _ = socket.send(Message::Close(Some(frame))).await;
}
