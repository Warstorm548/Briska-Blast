use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use rand::Rng;
use shared::types::player::PlayerId;
use shared::protocol::messages::RegisterResponse;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, hash_token};

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<RegisterResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_register.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let counter: u64 = conn
        .incr("player:counter", 1u64)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let player_id = PlayerId::from_counter(counter).to_string();

    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    let secret_token = hex::encode(token_bytes);
    let token_hash = hash_token(&secret_token);

    conn.set::<_, _, ()>(format!("player:{}:token_hash", player_id), &token_hash)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!("registered player {}", player_id);

    Ok(Json(RegisterResponse {
        player_id,
        secret_token,
    }))
}
