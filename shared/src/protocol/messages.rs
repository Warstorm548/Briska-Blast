use serde::{Deserialize, Serialize};

use crate::types::gamemode::GameMode;
use crate::types::session::SessionStatus;

/// Sent by the launcher on every boot. `prior_*` are present when the launcher
/// has a cached identity for this channel — the server reuses the existing
/// player_id when the token hash matches, otherwise it falls through to a
/// fresh issuance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub prior_player_id: Option<String>,
    pub prior_secret_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub player_id: String,
    pub secret_token: String,
    /// Echo of the username the server now has on file for this player.
    pub username: String,
    /// Server-side per-user gate. The dev channel UI in the launcher is
    /// hidden unless this is `true` on the dev server's response — the
    /// launcher never persists this; the server is the source of truth.
    pub dev_flag: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUsernameRequest {
    pub player_id: String,
    pub secret_token: String,
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostRequest {
    pub player_id: String,
    pub secret_token: String,
    pub gamemode: GameMode,
    pub player_count: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostResponse {
    pub session_code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    pub session_code: String,
    pub player_id: String,
    pub secret_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinedPeer {
    pub player_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinResponse {
    pub gamemode: GameMode,
    pub player_count: u8,
    pub current_player_count: u8,
    pub joiners: Vec<JoinedPeer>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionPollResponse {
    pub status: SessionStatus,
    pub gamemode: GameMode,
    pub player_count: u8,
    pub current_player_count: u8,
    pub joiner_player_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloseSessionRequest {
    pub player_id: String,
    pub secret_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartSessionRequest {
    pub player_id: String,
    pub secret_token: String,
}

/// Host voluntarily hands the host role to another player already in the
/// lobby. `player_id`/`secret_token` authenticate the *current* host;
/// `new_host_player_id` must be one of the session's joiners.
#[derive(Debug, Serialize, Deserialize)]
pub struct TransferHostRequest {
    pub player_id: String,
    pub secret_token: String,
    pub new_host_player_id: String,
}
