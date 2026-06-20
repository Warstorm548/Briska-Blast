//! WebSocket signaling endpoint. This module is the lifecycle orchestrator: it
//! upgrades the connection ([`ws_handler`]), runs the identify → register →
//! pump → cleanup phases ([`handle_socket`]), and owns the shared close-code
//! vocabulary. The per-phase work lives in the submodules:
//!
//!   identify      Phase-1 auth + peer-roster snapshot
//!   frame         Phase-3 inbound client-frame routing
//!   disconnect    grace windows / reconnect holds armed during cleanup
//!   session_ops   atomic Redis Lua mutations (end / promote / remove)
//!
//! No behavior or logic differs from the single-file version it was split from —
//! the public `crate::signaling::ws::ws_handler` surface is preserved.

mod disconnect;
mod frame;
mod identify;
mod session_ops;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, State,
    },
    response::IntoResponse,
};
use std::borrow::Cow;
use std::net::SocketAddr;

use crate::{
    api::fetch_usernames,
    signaling::{protocol::ServerMsg, GraceKind},
    state::AppState,
};

use disconnect::{
    arm_host_disconnect_grace, arm_joiner_disconnect_grace, broadcast_peer_left,
    session_status_is_active,
};
use frame::handle_client_frame;
use identify::{identify, peer_roster, RosterSnapshot};
use session_ops::{
    end_session_if_waiting, promote_demote_or_end_active, remove_joiner_on_leave,
    HostDisconnectStage,
};

// App-defined close codes in the WebSocket 4xxx range. Chosen to mirror
// the HTTP semantics a launcher developer already knows from REST.
const CLOSE_BAD_INITIAL: u16 = 4400;
const CLOSE_UNAUTHORIZED: u16 = 4401;
const CLOSE_FORBIDDEN: u16 = 4403;
const CLOSE_NOT_FOUND: u16 = 4404;
const CLOSE_INTERNAL: u16 = 4500;

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

    // Snapshot the roster at identify time so the client doesn't need a poll.
    // `peers` excludes self (mesh targets); `seat_order` is the frozen seating
    // roster (empty until Start) the client uses to lay out Extended-mode portals.
    let RosterSnapshot { peers, seat_order } = match peer_roster(&state, &code, &player_id).await {
        Ok(r) => r,
        Err(_) => {
            // Session must have been deleted between membership check
            // and now — vanishingly rare race, but bail cleanly.
            close_with(&mut socket, CLOSE_NOT_FOUND, "session_gone").await;
            state.signal_hub.leave_room(&code, &player_id, conn_id).await;
            return;
        }
    };

    // Resolve display names for everyone named in this frame (self, host, peers)
    // so the client labels the lobby/scoreboard by username rather than the
    // internal id. peer_roster already returned its own conn, so grab a fresh
    // one; a failure degrades to an empty map (clients fall back to `Player <id>`).
    let usernames = match state.redis.get().await {
        Ok(mut conn) => {
            let mut ids = Vec::with_capacity(peers.len() + 2);
            ids.push(player_id.clone());
            ids.push(host_player_id.clone());
            ids.extend(peers.iter().cloned());
            fetch_usernames(&mut conn, &ids).await
        }
        Err(_) => std::collections::HashMap::new(),
    };

    // The joining player's own name, reused for the PeerJoined broadcast below so
    // existing members can label the newcomer without another lookup. Empty when
    // unknown — peers then fall back to `Player <id>`.
    let self_username = usernames.get(&player_id).cloned().unwrap_or_default();

    let identified = ServerMsg::Identified {
        your_player_id: player_id.clone(),
        host_player_id,
        peers,
        seat_order,
        is_host,
        usernames,
    };
    if let Ok(text) = serde_json::to_string(&identified) {
        let _ = socket.send(Message::Text(text)).await;
    }

    state
        .signal_hub
        .broadcast(
            &code,
            ServerMsg::PeerJoined {
                player_id: player_id.clone(),
                username: self_username,
            },
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

async fn close_with(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: Cow::Borrowed(reason),
    };
    let _ = socket.send(Message::Close(Some(frame))).await;
}
