use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use shared::protocol::messages::{JoinRequest, JoinResponse};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player, Session};

pub async fn join(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<JoinRequest>,
) -> Result<Json<JoinResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_join.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let key = format!("session:{}", body.session_code);
    let raw: Option<String> = conn
        .get(&key)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw = raw.ok_or(AppError::NotFound)?;

    let mut session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if session.status != "waiting" {
        return Err(AppError::Conflict("session_already_active".to_string()));
    }

    let host_ip = session.host_ip.clone();
    let host_port = session.host_port;

    session.joiner_player_id = Some(body.player_id.clone());
    session.joiner_ip = Some(body.external_ip);
    session.joiner_port = Some(body.external_port);
    session.status = "active".to_string();

    let updated = serde_json::to_string(&session)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    conn.set_ex::<_, _, ()>(&key, updated, state.config.session_ttl_secs)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(
        "player {} joined session {}",
        body.player_id, body.session_code
    );

    Ok(Json(JoinResponse { host_ip, host_port }))
}
