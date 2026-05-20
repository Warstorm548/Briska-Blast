use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use shared::protocol::messages::StartSessionRequest;
use shared::types::session::SessionStatus;
use std::collections::HashSet;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    gamemode,
    signaling::ServerMsg,
    state::AppState,
};
use super::{client_ip, validate_player, Session};

pub async fn start_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<StartSessionRequest>,
) -> Result<StatusCode> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let key = format!("session:{}", code);
    let raw: Option<String> = conn
        .get(&key)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let raw = raw.ok_or(AppError::NotFound)?;
    let mut session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Precondition: caller is the host. Defense in depth — the UI hides the
    // start button for non-hosts, but the server doesn't trust that.
    if session.host_player_id != body.player_id {
        return Err(AppError::SessionNotStartable { reason: "not_host" });
    }

    // Precondition: session is still in the lobby phase. Re-starting a
    // Starting/Active/Ended session is an error, not a no-op.
    if !matches!(session.status, SessionStatus::Waiting) {
        return Err(AppError::SessionNotStartable { reason: "not_in_waiting" });
    }

    // Precondition: at least gamemode-minimum humans in the lobby.
    let (min_players, _max_players) = gamemode::bounds_for(session.gamemode);
    if session.current_player_count() < min_players {
        return Err(AppError::SessionNotStartable {
            reason: "below_min_players",
        });
    }

    // Precondition: every session member has a live WS connection in
    // SignalHub. Without this, start_signaling would broadcast to a
    // partial roster and the missing player would never establish peer
    // links. Strict by design (per plan default).
    let ws_members: HashSet<String> =
        state.signal_hub.room_members(&code).await.into_iter().collect();
    let session_members: Vec<String> = std::iter::once(session.host_player_id.clone())
        .chain(session.joiners.iter().map(|j| j.player_id.clone()))
        .collect();
    if !session_members.iter().all(|p| ws_members.contains(p)) {
        return Err(AppError::SessionNotStartable {
            reason: "not_all_peers_ready",
        });
    }

    // Persist the transition BEFORE broadcasting. If we broadcast first and
    // the SET fails, clients would already be negotiating WebRTC against a
    // session that still reads as Waiting — recoverable but ugly. Doing the
    // persist first keeps the visible session state and the signaling
    // intent aligned.
    session.status = SessionStatus::Starting;
    let updated = serde_json::to_string(&session)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    conn.set_ex::<_, _, ()>(&key, updated, state.config.session_ttl_secs)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Best-effort broadcast. Clients that miss it can re-poll /session/:code
    // to discover the new Starting status.
    state
        .signal_hub
        .broadcast(
            &code,
            ServerMsg::StartSignaling {
                gamemode: session.gamemode,
                player_count: session.player_count,
                peers: session_members,
            },
            None,
        )
        .await;

    tracing::info!(
        "player {} started session {} ({}/{} players)",
        body.player_id,
        code,
        session.current_player_count(),
        session.player_count
    );

    Ok(StatusCode::NO_CONTENT)
}
