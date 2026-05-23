//! Thin HTTP client for the per-channel server. Runs `/register` (idempotent;
//! refreshes the dev_flag) on every boot and `/me/username` when the user
//! commits a name change. All calls go over `https://<channel.host()>/<path>`
//! on the default port — hostname table is in `channel::Channel::host()`.

use crate::channel::Channel;
use shared::protocol::messages::{RegisterRequest, RegisterResponse, UpdateUsernameRequest};
use std::sync::OnceLock;
use std::time::Duration;

/// Process-wide reqwest client. Initialised once on first call; reuse keeps
/// the underlying connection pool warm across the per-launch /register
/// fan-out and any subsequent /me/username calls, so we don't pay TLS setup
/// every time. Build failure here is a "machine is broken" condition (TLS
/// init); panicking is acceptable.
fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client builder")
    })
}

pub async fn register(
    channel: Channel,
    req: RegisterRequest,
) -> Result<RegisterResponse, String> {
    let url = format!("https://{}/register", channel.host());
    let resp = http()
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
    let resp = http()
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
