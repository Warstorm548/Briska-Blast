use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use shared::protocol::messages::{UpdateUsernameRequest, UpdateUsernameResponse, MAX_USERNAME_LEN};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player};

pub async fn update_username(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<UpdateUsernameRequest>,
) -> Result<impl IntoResponse> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_me_username.check_key(&ip).is_err() {
        tracing::warn!(client = %ip, "username change rate-limited");
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Authenticate before the length check: a rejection echoes the caller's own
    // stored name back, so we must know who they are first (and never leak it on
    // bad creds — that path 401s here).
    validate_player(&mut conn, &req.player_id, &req.secret_token).await?;

    // The last stable username — the value a rejected change must NOT overwrite,
    // and which we hand back so the launcher can snap its UI to it.
    let stored: Option<String> = conn
        .get(format!("player:{}:username", req.player_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let stored = stored.unwrap_or_default();

    let username = req.username.trim().to_string();
    if username.is_empty() || username.chars().count() > MAX_USERNAME_LEN {
        // Trust-boundary reject: the launcher caps input at MAX_USERNAME_LEN, so
        // reaching here means a tampered/raw client. Leave Redis untouched and
        // return the stored name (422) so the launcher reverts to it.
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(UpdateUsernameResponse { username: stored }),
        ));
    }

    conn.set::<_, _, ()>(format!("player:{}:username", req.player_id), &username)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok((StatusCode::OK, Json(UpdateUsernameResponse { username })))
}
