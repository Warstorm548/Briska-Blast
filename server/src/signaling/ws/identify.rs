//! Phase-1 of a signaling connection: read the first frame, authenticate the
//! token, and confirm the player belongs to the session before the handler in
//! [`super`] starts pumping messages. Also the peer-roster snapshot returned in
//! the `Identified` frame so a freshly-identified client needs no extra poll.

use axum::extract::ws::{Message, WebSocket};
use deadpool_redis::redis::AsyncCommands;
use std::time::Duration;

use super::{
    close_with, CLOSE_BAD_INITIAL, CLOSE_FORBIDDEN, CLOSE_INTERNAL, CLOSE_NOT_FOUND,
    CLOSE_UNAUTHORIZED,
};
use crate::{
    api::{validate_player, Session},
    signaling::protocol::ClientMsg,
    state::AppState,
};

// The first frame after upgrade must arrive within this window or the
// server closes the connection. Prevents a malicious or buggy client
// from pinning a connection that never identifies.
const IDENTIFY_DEADLINE: Duration = Duration::from_secs(5);

/// Reads the first frame, validates the token, confirms membership.
/// Closes the socket with the appropriate 4xxx code on any failure.
/// On success returns `(player_id, is_host, host_player_id, rejoin)` —
/// `rejoin` is the client's own process-death-rejoin declaration (see
/// `ClientMsg::Identify`), which the caller uses to trigger pause-on-rejoin.
pub(super) async fn identify(
    socket: &mut WebSocket,
    code: &str,
    state: &AppState,
) -> Result<(String, bool, String, bool), ()> {
    let first = match tokio::time::timeout(IDENTIFY_DEADLINE, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            close_with(socket, CLOSE_BAD_INITIAL, "identify_required").await;
            return Err(());
        }
    };

    let (player_id, secret_token, rejoin) = match serde_json::from_str::<ClientMsg>(&first) {
        Ok(ClientMsg::Identify { player_id, secret_token, rejoin }) => {
            (player_id, secret_token, rejoin)
        }
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
    Ok((player_id, is_host, session.host_player_id, rejoin))
}

/// Session roster snapshot for the Identified frame, so the client needs no
/// separate poll. `peers` excludes the requesting player (used to mesh);
/// `seat_order` is the frozen, self-inclusive seating roster (empty until Start).
pub(super) struct RosterSnapshot {
    pub peers: Vec<String>,
    pub seat_order: Vec<String>,
}

pub(super) async fn peer_roster(
    state: &AppState,
    code: &str,
    self_player_id: &str,
) -> Result<RosterSnapshot, ()> {
    let mut conn = state.redis.get().await.map_err(|_| ())?;
    let raw: Option<String> = conn.get(format!("session:{}", code)).await.map_err(|_| ())?;
    let raw = raw.ok_or(())?;
    let session: Session = serde_json::from_str(&raw).map_err(|_| ())?;

    let peers = std::iter::once(session.host_player_id.clone())
        .chain(session.joiners.iter().map(|j| j.player_id.clone()))
        .filter(|p| p != self_player_id)
        .collect();
    Ok(RosterSnapshot {
        peers,
        seat_order: session.seat_order,
    })
}
