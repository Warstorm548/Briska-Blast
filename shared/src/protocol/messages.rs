use serde::{Deserialize, Serialize};

use crate::types::gamemode::GameMode;
use crate::types::session::SessionStatus;

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub player_id: String,
    pub secret_token: String,
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
