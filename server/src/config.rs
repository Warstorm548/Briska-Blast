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
                .unwrap_or_else(|_| "http://watchtower:8080".to_string()),
            watchtower_token: env::var("WATCHTOWER_TOKEN")
                .unwrap_or_else(|_| "briska-watchtower-token".to_string()),
        }
    }
}
