use std::env;

pub struct Config {
    pub redis_url: String,
    pub game_port: u16,
    pub admin_port: u16,
    pub session_ttl_secs: u64,
    pub min_launcher_version: String,
    pub min_game_version: String,
    pub admin_password: String,
    pub watchtower_url: String,
    pub watchtower_token: String,
    /// Log output format: `json` for machine-parseable lines (feeds the future
    /// admin Logs tab / log shippers), anything else for human-readable `pretty`.
    pub log_format: String,
    /// Cloudflare TURN key id + API token (dashboard: Realtime → TURN keys).
    /// Both empty ⇒ TURN minting is disabled and clients fall back to their
    /// built-in STUN-only config — playable on friendly NATs, so absence warns
    /// at boot instead of panicking (contrast WATCHTOWER_TOKEN above). The
    /// token never leaves the server: clients only ever see the short-lived
    /// credentials minted from it (see `turn.rs`).
    pub turn_key_id: String,
    pub turn_api_token: String,

    /// Pocket ID OIDC login for the admin panel (see `admin/oidc.rs`). All
    /// four empty ⇒ OIDC is disabled and the panel falls back to plain
    /// password login (fail-open, same posture as TURN above). Never panics
    /// on absence, so a local/dev server runs with no Pocket ID configured.
    /// `admin_public_url` is the panel's externally-reachable origin — the
    /// OIDC callback is *derived* from it so it can't drift from what's
    /// registered in Pocket ID. `oidc_client_secret` never leaves the server.
    pub admin_public_url: String,
    pub oidc_issuer_url: String,
    pub oidc_client_id: String,
    pub oidc_client_secret: String,

    /// Optional secret *pepper* for the break-glass admin password. When set,
    /// the password is HMAC-SHA256'd with this key before bcrypt (see
    /// `admin::hash_break_glass`), so a Redis-only compromise can't offline
    /// brute-force the hash. Empty ⇒ plain bcrypt (backward compatible with
    /// existing stored hashes). NOTE: changing it invalidates the stored hash
    /// — rotating requires re-seeding the password. Never leaves the server.
    pub break_glass_pepper: String,

    /// Prefix for the three admin group names Pocket ID must contain, so each
    /// deployment (dev/ea/stable) gates on DISTINCT groups. The server
    /// authorizes against `{prefix}superadmin` / `{prefix}admin` /
    /// `{prefix}moderator`. Default `briska-`.
    pub oidc_group_prefix: String,

    /// Secret pepper (HMAC key) for the admin Logs tab's client-IP `⟨ip:…⟩`
    /// tokens (`redact.rs`), applied only at the web render boundary. Empty ⇒
    /// `AppState` uses a random per-boot key, so tokens are never a weak unkeyed
    /// hash; setting it only keeps tokens STABLE ACROSS RESTARTS — which matters
    /// solely for persisted/compared tokens (saved Download `.log`s or a future
    /// audit stream). Nothing today depends on it. Never leaves the server.
    pub ip_hash_pepper: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        let game_port = match env::var("GAME_PORT") {
            Ok(v) => v.parse::<u16>().unwrap_or_else(|_| {
                panic!("invalid GAME_PORT '{v}': expected an integer in 0..=65535")
            }),
            Err(_) => 25919,
        };
        let admin_port = match env::var("ADMIN_PORT") {
            Ok(v) => v.parse::<u16>().unwrap_or_else(|_| {
                panic!("invalid ADMIN_PORT '{v}': expected an integer in 0..=65535")
            }),
            Err(_) => 25920,
        };

        Self {
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            game_port,
            admin_port,
            session_ttl_secs: env::var("SESSION_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1800),
            min_launcher_version: env::var("MIN_LAUNCHER_VERSION")
                .unwrap_or_else(|_| "0.1.0".to_string()),
            min_game_version: env::var("MIN_GAME_VERSION")
                .unwrap_or_else(|_| "0.1.0".to_string()),
            admin_password: env::var("ADMIN_PASSWORD")
                .unwrap_or_else(|_| "@admin".to_string()),
            watchtower_url: env::var("WATCHTOWER_URL")
                .unwrap_or_else(|_| "http://watchtower:25921".to_string()),
            // No fallback: docker-compose.yml already enforces this via
            // `${WATCHTOWER_TOKEN:?...}`, and the binary must enforce the
            // same fail-closed posture for non-compose runs (dev shell,
            // manual deploy, custom orchestrator). A hardcoded literal
            // here would otherwise let a missing .env silently boot with
            // a publicly-known token.
            watchtower_token: env::var("WATCHTOWER_TOKEN").unwrap_or_else(|_| {
                panic!(
                    "WATCHTOWER_TOKEN must be set — set it in .env or your container environment. \
                     There is no fallback because that would let a missing config silently run \
                     with a known-literal token."
                )
            }),
            log_format: env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string()),
            turn_key_id: env::var("TURN_KEY_ID").unwrap_or_default(),
            turn_api_token: env::var("TURN_API_TOKEN").unwrap_or_default(),
            admin_public_url: env::var("ADMIN_PUBLIC_URL").unwrap_or_default(),
            oidc_issuer_url: env::var("OIDC_ISSUER_URL").unwrap_or_default(),
            oidc_client_id: env::var("OIDC_CLIENT_ID").unwrap_or_default(),
            oidc_client_secret: env::var("OIDC_CLIENT_SECRET").unwrap_or_default(),
            break_glass_pepper: env::var("BREAK_GLASS_PEPPER").unwrap_or_default(),
            oidc_group_prefix: env::var("OIDC_GROUP_PREFIX")
                .unwrap_or_else(|_| "briska-".to_string()),
            ip_hash_pepper: env::var("IP_HASH_PEPPER").unwrap_or_default(),
        }
    }

    /// True only when every OIDC field is present. Gates the "Sign in with
    /// Pocket ID" button and the callback route; when false the panel is
    /// plain-password-only (fail-open).
    pub fn oidc_enabled(&self) -> bool {
        !self.admin_public_url.trim().is_empty()
            && !self.oidc_issuer_url.trim().is_empty()
            && !self.oidc_client_id.trim().is_empty()
            && !self.oidc_client_secret.trim().is_empty()
    }

    /// The OIDC redirect/callback URL, derived from the panel's public origin
    /// so it always matches whatever is entered here — register the *same*
    /// value in the Pocket ID client's Callback URLs.
    pub fn oidc_redirect_uri(&self) -> String {
        format!(
            "{}/admin/oidc/callback",
            self.admin_public_url.trim_end_matches('/')
        )
    }

    /// This server's admin group names, in `[SuperAdmin, Admin, Moderator]`
    /// order. Derived from `oidc_group_prefix` so each deployment gates on
    /// distinct groups.
    pub fn oidc_group_names(&self) -> [String; 3] {
        [
            format!("{}superadmin", self.oidc_group_prefix),
            format!("{}admin", self.oidc_group_prefix),
            format!("{}moderator", self.oidc_group_prefix),
        ]
    }
}
