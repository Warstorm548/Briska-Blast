pub mod host;
pub mod join;
pub mod register;
pub mod session;

use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub code: String,
    pub host_player_id: String,
    pub host_ip: String,
    pub host_port: u16,
    pub joiner_player_id: Option<String>,
    pub joiner_ip: Option<String>,
    pub joiner_port: Option<u16>,
    pub status: String,
}

pub(crate) fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) async fn validate_player(
    conn: &mut deadpool_redis::Connection,
    player_id: &str,
    secret_token: &str,
) -> Result<()> {
    let stored: Option<String> = conn
        .get(format!("player:{}:token_hash", player_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let stored = stored.ok_or(AppError::Unauthorized)?;

    if hash_token(secret_token) != stored {
        return Err(AppError::Unauthorized);
    }

    Ok(())
}

pub(crate) fn client_ip(
    connect_info: &ConnectInfo<SocketAddr>,
    headers: &axum::http::HeaderMap,
) -> IpAddr {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| connect_info.0.ip())
}
