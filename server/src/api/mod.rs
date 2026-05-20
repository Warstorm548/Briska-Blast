pub mod host;
pub mod join;
pub mod register;
pub mod session;

use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::types::gamemode::GameMode;
use shared::types::session::SessionStatus;
use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinerEntry {
    pub player_id: String,
    pub joined_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub code: String,
    pub host_player_id: String,
    pub gamemode: GameMode,
    pub player_count: u8,
    pub joiners: Vec<JoinerEntry>,
    pub status: SessionStatus,
}

impl Session {
    pub fn current_player_count(&self) -> u8 {
        // Host plus joiners. Joiner count is capped at u8::MAX in practice
        // because player_count is u8 and is_full() enforces the upper bound
        // before any append.
        1u8.saturating_add(self.joiners.len() as u8)
    }

    pub fn is_full(&self) -> bool {
        self.current_player_count() >= self.player_count
    }

    pub fn contains_player(&self, player_id: &str) -> bool {
        self.host_player_id == player_id
            || self.joiners.iter().any(|j| j.player_id == player_id)
    }
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
