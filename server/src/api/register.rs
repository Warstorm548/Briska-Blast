use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use rand::Rng;
use shared::protocol::messages::{RegisterRequest, RegisterResponse};
use shared::types::player::PlayerId;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, hash_token};

/// Cap username at 32 chars to match the launcher UI input; trimmed of
/// surrounding whitespace before storage so the admin Users tab sees
/// canonical values.
const MAX_USERNAME_LEN: usize = 32;

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_register.check_key(&ip).is_err() {
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

    // Try to reuse the prior identity. If anything about the lookup fails or
    // the token hash doesn't match, fall through to fresh issuance — the
    // launcher must be able to recover from a corrupted identity file.
    let reused = match (&req.prior_player_id, &req.prior_secret_token) {
        (Some(pid), Some(token)) if !pid.is_empty() && !token.is_empty() => {
            let stored: Option<String> = conn
                .get(format!("player:{}:token_hash", pid))
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            match stored {
                Some(h) if h == hash_token(token) => Some((pid.clone(), token.clone())),
                _ => None,
            }
        }
        _ => None,
    };

    let (player_id, secret_token) = match reused {
        Some(pair) => pair,
        None => {
            let counter: u64 = conn
                .incr("player:counter", 1u64)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let pid = PlayerId::from_counter(counter).to_string();
            let token_bytes: [u8; 32] = rand::thread_rng().gen();
            let token = hex::encode(token_bytes);
            let hash = hash_token(&token);

            conn.set::<_, _, ()>(format!("player:{}:token_hash", pid), &hash)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            tracing::info!("registered player {}", pid);
            (pid, token)
        }
    };

    // Refresh username in Redis on every register — username can change at
    // any time on the launcher and the server is the canonical record.
    conn.set::<_, _, ()>(format!("player:{}:username", player_id), &username)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let dev_flag: bool = conn
        .get::<_, Option<String>>(format!("player:{}:dev_flag", player_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_deref()
        == Some("true");

    Ok(Json(RegisterResponse {
        player_id,
        secret_token,
        username,
        dev_flag,
    }))
}
