use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub player_id: String,
    pub secret_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostRequest {
    pub player_id: String,
    pub secret_token: String,
    pub external_ip: String,
    pub external_port: u16,
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
    pub external_ip: String,
    pub external_port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinResponse {
    pub host_ip: String,
    pub host_port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionPollResponse {
    pub status: String,
    pub joiner_ip: Option<String>,
    pub joiner_port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloseSessionRequest {
    pub player_id: String,
    pub secret_token: String,
}
