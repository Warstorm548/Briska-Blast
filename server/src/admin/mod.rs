pub mod auth;
pub mod dashboard;
pub mod stats;
pub(crate) mod templates;
pub mod users;

use axum::http::HeaderMap;
use deadpool_redis::{redis::AsyncCommands, Pool};
use serde::Deserialize;

// Admin idle-session policy. Single source of truth: the server uses
// `ADMIN_SESSION_TTL_SECS` for the Redis key TTL, and the client idle timer is
// rendered from `ADMIN_IDLE_WARN_SECS` / `ADMIN_IDLE_LOGOUT_SECS` (injected into
// the panel JS), so the two halves can never drift.
//
// A *live* browser is logged out at exactly LOGOUT (the client POSTs
// /admin/logout itself). The longer server TTL is only the backstop for a tab
// where JS isn't running (crashed / slept): TTL = LOGOUT + keepalive throttle +
// margin, so a last-second "Keep me logged in" click can't be wrongly rejected.
pub const ADMIN_IDLE_WARN_SECS: u64 = 300; // 5:00 — client shows the warning modal
pub const ADMIN_IDLE_LOGOUT_SECS: u64 = 330; // 5:30 — client force-logout
pub const ADMIN_SESSION_TTL_SECS: u64 = 420; // 7:00 — server-side hard backstop

pub async fn require_session(headers: &HeaderMap, redis: &Pool) -> Option<String> {
    let cookie = headers.get("cookie")?.to_str().ok()?;
    let token = cookie
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with("briska_admin_session="))
        .map(|s| s.trim_start_matches("briska_admin_session=").to_string())?;

    let mut conn = redis.get().await.ok()?;
    // EXPIRE returns false when the key is already gone, so it doubles as the
    // existence check while sliding the idle window forward on every
    // authenticated request — i.e. any admin activity counts as "still active".
    let refreshed: bool = conn
        .expire(
            format!("admin:session:{}", token),
            ADMIN_SESSION_TTL_SECS as i64,
        )
        .await
        .ok()?;
    if refreshed { Some(token) } else { None }
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
pub struct PasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}

#[derive(Deserialize)]
pub struct UpdateSettingsForm {
    pub auto_enabled: Option<String>,         // "on" when checkbox checked, absent otherwise
    pub check_interval_secs: Option<String>,
    pub apply_interval_secs: Option<String>,
}

#[derive(Deserialize)]
pub struct ScheduleUpdateForm {
    pub scheduled_at: String, // datetime-local value: "2026-05-20T03:00"
}

#[derive(Deserialize)]
pub struct RollbackForm {
    pub version: String, // e.g. "v0.3.0"
}
