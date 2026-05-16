use std::env;

pub struct Config {
    pub redis_url: String,
    pub game_port: u16,
    pub admin_port: u16,
    pub session_ttl_secs: u64,
    pub min_launcher_version: String,
    pub min_game_version: String,
    pub admin_password: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            game_port: env::var("GAME_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(25919),
            admin_port: env::var("ADMIN_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(25920),
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
        }
    }
}
