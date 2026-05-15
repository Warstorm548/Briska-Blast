pub mod auth;
pub mod dashboard;
pub(crate) mod templates;

use axum::http::HeaderMap;
use deadpool_redis::{redis::AsyncCommands, Pool};
use serde::Deserialize;

pub async fn require_session(headers: &HeaderMap, redis: &Pool) -> Option<String> {
    let cookie = headers.get("cookie")?.to_str().ok()?;
    let token = cookie
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with("briska_admin_session="))
        .map(|s| s.trim_start_matches("briska_admin_session=").to_string())?;

    let mut conn = redis.get().await.ok()?;
    let exists: bool = conn
        .exists(format!("admin:session:{}", token))
        .await
        .ok()?;
    if exists { Some(token) } else { None }
}

pub fn set_cookie(token: &str) -> String {
    format!(
        "briska_admin_session={}; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=86400",
        token
    )
}

pub fn clear_cookie() -> &'static str {
    "briska_admin_session=; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=0"
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub password: String,
}

#[derive(Deserialize)]
pub struct VersionForm {
    pub version: String,
}

#[derive(Deserialize)]
pub struct BindAddrForm {
    pub bind_addr: String,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}
