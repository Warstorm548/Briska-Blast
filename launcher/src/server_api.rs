//! Thin HTTP client for the per-channel server. Runs `/register` (idempotent;
//! refreshes the dev_flag) on every boot and `/me/username` when the user
//! commits a name change. All calls go over `https://<channel.host()>/<path>`
//! on the default port — hostname table is in `channel::Channel::host()`.

use crate::channel::Channel;
use shared::protocol::messages::{RegisterRequest, RegisterResponse, UpdateUsernameRequest};
use std::time::Duration;

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

pub async fn register(
    channel: Channel,
    req: RegisterRequest,
) -> Result<RegisterResponse, String> {
    let url = format!("https://{}/register", channel.host());
    let resp = client()?
        .post(&url)
        .json(&req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("register {}: HTTP {}", channel.label(), resp.status()));
    }
    resp.json::<RegisterResponse>()
        .await
        .map_err(|e| e.to_string())
}

pub async fn update_username(
    channel: Channel,
    req: UpdateUsernameRequest,
) -> Result<(), String> {
    let url = format!("https://{}/me/username", channel.host());
    let resp = client()?
        .post(&url)
        .json(&req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "update_username {}: HTTP {}",
            channel.label(),
            resp.status()
        ));
    }
    Ok(())
}
