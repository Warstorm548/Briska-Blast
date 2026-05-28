use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, State,
    },
    response::IntoResponse,
};
use deadpool_redis::redis::AsyncCommands;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::time::Duration;

use crate::{
    api::{validate_player, Session},
    signaling::protocol::{ClientMsg, ServerMsg},
    state::AppState,
};

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
    state
        .signal_hub
        .broadcast(
            &code,
            ServerMsg::PeerLeft {
                player_id: player_id.clone(),
                reason: if explicit_leave { "leave" } else { "disconnect" },
            },
            None,
        )
        .await;

    if is_host {
        // If the host disconnected while the session was still in Waiting,
        // tear the whole lobby down — otherwise joiners would sit on a
        // defunct session until TTL. Joiner-list mutation in Starting/Active
        // is deferred (it races with /start and warrants its own commit).
        if let Err(e) = end_session_if_waiting(&state, &code).await {
            tracing::warn!("ws: host-disconnect cleanup failed for {}: {}", code, e);
        }
    } else if explicit_leave {
        // A joiner deliberately left. Free their Redis slot so the lobby's
        // capacity is correct and /start's "all peers ready" check can pass
        // for the remaining members. Only on an explicit leave AND only in
        // Waiting — a transient socket drop keeps the slot for reconnect,
        // and a leave mid-game is the deferred auto-promotion concern.
        if let Err(e) = remove_joiner_if_waiting(&state, &code, &player_id).await {
            tracing::warn!("ws: joiner-leave cleanup failed for {} in {}: {}", player_id, code, e);
        }
    }

    tracing::info!("ws: player {} disconnected from session {}", player_id, code);
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

/// On host disconnect, end the session and notify remaining peers — but
/// only if the session is still in Waiting. Past Waiting, the match has
/// already started and host-loss is a game-state concern handled elsewhere.
///
/// Atomicity matters here: a non-atomic GET-then-DEL races with
/// `/session/:code/start` transitioning Waiting → Starting between the
/// two calls. A single Lua script performs the read, status check, and
/// conditional DEL as one Redis operation — no other command can
/// interleave.
async fn end_session_if_waiting(state: &AppState, code: &str) -> Result<(), String> {
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

    if !matches!(outcome, EndOutcome::Deleted) {
        // Nothing to clean up — either the session was already gone or
        // it had already advanced past Waiting (likely a concurrent
        // /start). No broadcast needed in either case.
        return Ok(());
    }

    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::SessionEnded { reason: "host_disconnect" },
            None,
        )
        .await;

    tracing::info!("ws: session {} ended (host disconnected during waiting)", code);
    Ok(())
}

/// On an explicit joiner leave, remove that joiner from the session's
/// Redis roster — but only while the session is still in Waiting. Past
/// Waiting the match has started and roster mutation is the deferred
/// auto-promotion concern.
///
/// Atomic for the same reason `end_session_if_waiting` is: a non-atomic
/// GET-then-SET races with `/session/:code/start`. Here the single Lua
/// script reads, status-checks, removes, and writes as one operation.
/// `/start`'s own CAS (its `expected_joiner_count` guard) handles the
/// cross-operation race — if this removal lands between /start's WS-ready
/// check and its transaction, /start sees the count mismatch and retries.
async fn remove_joiner_if_waiting(
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
    const REMOVE_JOINER_IF_WAITING_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_waiting'})
end
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
        NotFound,
        NotWaiting,
        NotJoiner,
    }

    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(REMOVE_JOINER_IF_WAITING_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .arg(player_id)
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: RemoveOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed lua result: {e}"))?;

    if matches!(outcome, RemoveOutcome::Removed) {
        tracing::info!("ws: removed joiner {} from waiting session {}", player_id, code);
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
