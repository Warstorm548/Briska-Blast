use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use shared::protocol::messages::{CloseSessionRequest, SessionPollResponse};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player, Session};

pub async fn get_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<SessionPollResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw: Option<String> = conn
        .get(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw = raw.ok_or(AppError::NotFound)?;

    let session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let current_player_count = session.current_player_count();
    let joiner_player_ids = session
        .joiners
        .iter()
        .map(|j| j.player_id.clone())
        .collect();

    Ok(Json(SessionPollResponse {
        status: session.status,
        gamemode: session.gamemode,
        player_count: session.player_count,
        current_player_count,
        joiner_player_ids,
    }))
}

pub async fn close_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<CloseSessionRequest>,
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

    let raw: Option<String> = conn
        .get(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw = raw.ok_or(AppError::NotFound)?;

    let session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if session.host_player_id != body.player_id {
        return Err(AppError::Unauthorized);
    }

    conn.del::<_, ()>(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!("player {} closed session {}", body.player_id, code);

    Ok(StatusCode::NO_CONTENT)
}
