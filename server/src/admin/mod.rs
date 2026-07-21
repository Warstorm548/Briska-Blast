pub mod auth;
pub mod dashboard;
pub mod oidc;
pub mod stats;
pub(crate) mod templates;
pub mod users;

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use deadpool_redis::{
    redis::{AsyncCommands, Expiry},
    Pool,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

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

/// Redis flag: the stored break-glass password is the seeded, publicly-known
/// default (`@admin`) and must be rotated before it can grant access. Set at
/// seed time (default only), cleared by any password change.
pub const PASSWORD_MUST_ROTATE_KEY: &str = "admin:password_must_rotate";

/// Admin capability tier. Ordinal — `derive(Ord)` ranks variants in
/// declaration order, so `Moderator < Admin < SuperAdmin`; gate with
/// [`AdminRole::at_least`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminRole {
    Moderator,
    Admin,
    SuperAdmin,
}

impl AdminRole {
    /// True when this role is at least `min` — the floor used by handler guards.
    pub fn at_least(self, min: AdminRole) -> bool {
        self >= min
    }

    /// Human label for the nav bar and logs.
    pub fn label(self) -> &'static str {
        match self {
            AdminRole::SuperAdmin => "SuperAdmin",
            AdminRole::Admin => "Admin",
            AdminRole::Moderator => "Moderator",
        }
    }
}

/// Map a `groups` claim to a role, highest wins. `names` is this server's
/// `[SuperAdmin, Admin, Moderator]` group names (see `Config::oidc_group_names`),
/// so each deployment gates on distinct groups. `None` ⇒ the user is in none of
/// this server's admin groups and must be denied.
pub fn role_from_groups(groups: &[String], names: &[String; 3]) -> Option<AdminRole> {
    let [superadmin, admin, moderator] = names;
    if groups.iter().any(|g| g == superadmin) {
        Some(AdminRole::SuperAdmin)
    } else if groups.iter().any(|g| g == admin) {
        Some(AdminRole::Admin)
    } else if groups.iter().any(|g| g == moderator) {
        Some(AdminRole::Moderator)
    } else {
        None
    }
}

/// An authenticated admin resolved from the session cookie. `username` is for
/// display (nav bar); `role` gates every action.
#[derive(Clone, Debug)]
pub struct AdminSession {
    pub token: String,
    pub role: AdminRole,
    pub username: String,
}

impl AdminSession {
    pub fn at_least(&self, min: AdminRole) -> bool {
        self.role.at_least(min)
    }
}

/// What we store in Redis under `admin:session:{token}`, replacing the old
/// literal `"1"`. A legacy `"1"` value fails to parse and is treated as
/// expired, forcing a one-time re-login after the upgrade.
#[derive(Serialize, Deserialize)]
struct SessionRecord {
    role: AdminRole,
    user: String,
    #[serde(default)]
    sub: String,
}

/// Where a logged-in-but-unauthorized admin is sent — they have a valid
/// session, just not the required role.
pub const FORBIDDEN_REDIRECT: &str =
    "/admin/dashboard?err=You+do+not+have+permission+for+that+action";

pub async fn require_session(headers: &HeaderMap, redis: &Pool) -> Option<AdminSession> {
    let cookie = headers.get("cookie")?.to_str().ok()?;
    let token = cookie
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with("briska_admin_session="))
        .map(|s| s.trim_start_matches("briska_admin_session=").to_string())?;

    let mut conn = redis.get().await.ok()?;
    // GETEX fetches the stored record AND slides the idle window forward in a
    // single round-trip (replacing the old EXPIRE-as-existence-check) — any
    // authenticated request counts as "still active". A missing key yields
    // None; a legacy `"1"` value fails to parse below and is rejected.
    let value: Option<String> = conn
        .get_ex(
            format!("admin:session:{}", token),
            Expiry::EX(ADMIN_SESSION_TTL_SECS as usize),
        )
        .await
        .ok()?;
    let record: SessionRecord = serde_json::from_str(&value?).ok()?;
    Some(AdminSession {
        token,
        role: record.role,
        username: record.user,
    })
}

/// Session guard with a role floor. `Ok` carries the session; `Err` is the
/// ready-made response to return — a login redirect when unauthenticated, the
/// forbidden redirect when the role is too low.
pub async fn require_role(
    headers: &HeaderMap,
    redis: &Pool,
    min: AdminRole,
) -> Result<AdminSession, Response> {
    match require_session(headers, redis).await {
        Some(session) if session.at_least(min) => Ok(session),
        Some(_) => Err(Redirect::to(FORBIDDEN_REDIRECT).into_response()),
        None => Err(Redirect::to("/admin").into_response()),
    }
}

/// Mint a fresh admin session for `role`/`user` and return its token. Used by
/// both the OIDC callback and the break-glass password login — the single
/// place that writes `admin:session:{token}`.
pub async fn create_session(
    redis: &Pool,
    role: AdminRole,
    user: &str,
    sub: &str,
) -> Option<String> {
    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    let token = hex::encode(token_bytes);
    let record = SessionRecord {
        role,
        user: user.to_string(),
        sub: sub.to_string(),
    };
    let value = serde_json::to_string(&record).ok()?;
    let mut conn = redis.get().await.ok()?;
    conn.set_ex::<_, _, ()>(
        format!("admin:session:{}", token),
        value,
        ADMIN_SESSION_TTL_SECS,
    )
    .await
    .ok()?;
    Some(token)
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

// ---- Break-glass password hashing: bcrypt + optional secret pepper ---------

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

/// Apply the secret pepper before bcrypt. An empty pepper is a pass-through,
/// so existing (un-peppered) hashes keep verifying. HMAC-SHA256 → 64 hex
/// chars, comfortably under bcrypt's 72-byte input limit (and free of the NUL
/// bytes a raw-binary MAC could contain, which bcrypt would truncate on).
fn prehash(pepper: &str, password: &str) -> String {
    if pepper.is_empty() {
        return password.to_string();
    }
    use hmac::Mac;
    let mut mac = HmacSha256::new_from_slice(pepper.as_bytes())
        .expect("HMAC-SHA256 accepts a key of any length");
    mac.update(password.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Hash the break-glass password for storage (bcrypt over the peppered input).
/// CPU-heavy — call inside `spawn_blocking`.
pub fn hash_break_glass(pepper: &str, password: &str) -> Result<String, bcrypt::BcryptError> {
    bcrypt::hash(prehash(pepper, password), bcrypt::DEFAULT_COST)
}

/// Verify a break-glass password against a stored hash. CPU-heavy — call
/// inside `spawn_blocking`.
pub fn verify_break_glass(pepper: &str, password: &str, hash: &str) -> bool {
    bcrypt::verify(prehash(pepper, password), hash).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering_is_moderator_admin_super() {
        assert!(AdminRole::SuperAdmin > AdminRole::Admin);
        assert!(AdminRole::Admin > AdminRole::Moderator);
        assert!(AdminRole::Admin.at_least(AdminRole::Moderator));
        assert!(AdminRole::SuperAdmin.at_least(AdminRole::SuperAdmin));
        assert!(!AdminRole::Moderator.at_least(AdminRole::Admin));
        assert!(!AdminRole::Admin.at_least(AdminRole::SuperAdmin));
    }

    #[test]
    fn role_from_groups_highest_wins_and_none_denied() {
        let names = |p: &str| {
            [
                format!("{p}superadmin"),
                format!("{p}admin"),
                format!("{p}moderator"),
            ]
        };
        let n = names("briska-");
        let g = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(role_from_groups(&g(&[]), &n), None);
        assert_eq!(role_from_groups(&g(&["some-other-group"]), &n), None);
        assert_eq!(role_from_groups(&g(&["briska-moderator"]), &n), Some(AdminRole::Moderator));
        assert_eq!(role_from_groups(&g(&["briska-admin"]), &n), Some(AdminRole::Admin));
        // superadmin wins even when all three groups are present
        assert_eq!(
            role_from_groups(
                &g(&["briska-moderator", "briska-admin", "briska-superadmin"]),
                &n
            ),
            Some(AdminRole::SuperAdmin)
        );
        // another deployment's groups (different prefix) grant nothing here —
        // this is the per-server isolation guarantee
        let ea = names("briska-ea-");
        assert_eq!(role_from_groups(&g(&["briska-superadmin"]), &ea), None);
    }

    #[test]
    fn session_record_roundtrips_and_legacy_value_rejected() {
        let rec = SessionRecord {
            role: AdminRole::Admin,
            user: "jean".into(),
            sub: "abc".into(),
        };
        let json = serde_json::to_string(&rec).unwrap();
        let back: SessionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, AdminRole::Admin);
        assert_eq!(back.user, "jean");
        // the pre-upgrade literal "1" must NOT parse as a session record
        assert!(serde_json::from_str::<SessionRecord>("\"1\"").is_err());
    }

    #[test]
    fn prehash_matches_rfc4231_hmac_sha256_vector() {
        // RFC 4231 test case 2: key="Jefe", data="what do ya want for nothing?"
        assert_eq!(
            prehash("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn pepper_empty_is_passthrough_but_nonempty_is_required_to_verify() {
        // empty pepper => plain bcrypt still verifies (backward compatible)
        let plain = hash_break_glass("", "hunter2").unwrap();
        assert!(verify_break_glass("", "hunter2", &plain));
        // with a pepper, the password only verifies under the SAME pepper
        let peppered = hash_break_glass("s3cret-pepper", "hunter2").unwrap();
        assert!(verify_break_glass("s3cret-pepper", "hunter2", &peppered));
        assert!(!verify_break_glass("", "hunter2", &peppered));
        assert!(!verify_break_glass("wrong-pepper", "hunter2", &peppered));
    }
}
