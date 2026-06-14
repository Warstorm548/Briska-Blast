pub mod host;
pub mod join;
pub mod me;
pub mod register;
pub mod session;
pub mod start;

use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::types::gamemode::GameMode;
use shared::types::session::SessionStatus;
use std::collections::HashMap;
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

/// Monotonic high-water counter for issued player ids. Only ever `INCR`'d —
/// never decremented — so the dashboard's "players registered" figure keeps
/// climbing even as ids are deleted and recycled.
pub(crate) const PLAYER_COUNTER_KEY: &str = "player:counter";

/// Sorted-set pool of freed player-id numbers (member == score == the numeric
/// counter value). Admin user-deletion pushes the freed number here; `/register`
/// pops the **lowest** one (`ZPOPMIN`) before falling back to the counter, so
/// deleted id numbers are reissued lowest-first to the next new player.
pub(crate) const FREELIST_KEY: &str = "player:freelist";

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

/// Resolve the server-stored display names for `ids` in one round-trip,
/// returning a `player_id -> username` map. Ids with no username on file are
/// **omitted** (so the client falls back to `Player <id>`), and a Redis error
/// degrades to an empty map rather than failing — a missing display name must
/// never break a signaling connection. Used to label the lobby roster and
/// in-game scoreboard by username instead of the internal numeric id.
pub(crate) async fn fetch_usernames(
    conn: &mut deadpool_redis::Connection,
    ids: &[String],
) -> HashMap<String, String> {
    if ids.is_empty() {
        return HashMap::new();
    }

    let keys: Vec<String> = ids
        .iter()
        .map(|id| format!("player:{}:username", id))
        .collect();

    let values: Vec<Option<String>> = match conn.mget(&keys).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("fetch_usernames: mget failed, falling back to ids: {}", e);
            return HashMap::new();
        }
    };

    // Zip ids back with their values; keep only ids that had a stored name.
    // Filtering nils here means the wire map never carries an empty string,
    // which the client would otherwise render as a blank name.
    ids.iter()
        .zip(values)
        .filter_map(|(id, name)| name.map(|n| (id.clone(), n)))
        .collect()
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
