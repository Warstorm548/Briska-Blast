use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use shared::protocol::messages::UpdateUsernameRequest;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player};

const MAX_USERNAME_LEN: usize = 32;

pub async fn update_username(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<UpdateUsernameRequest>,
) -> Result<impl IntoResponse> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_me_username.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let username = req.username.trim().to_string();
    if username.is_empty() || username.chars().count() > MAX_USERNAME_LEN {
        return Err(AppError::BadRequest("invalid username".into()));
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &req.player_id, &req.secret_token).await?;

    conn.set::<_, _, ()>(format!("player:{}:username", req.player_id), &username)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
