//! Cloudflare TURN credential minting.
//!
//! STUN-only ICE fails between symmetric/endpoint-dependent NATs (the
//! confirmed Win↔Mac field failure — both sides gather srflx, connectivity
//! checks never land). TURN fixes that with a relay both peers can dial
//! outbound. Rather than self-hosting coturn, the server mints short-lived
//! credentials from Cloudflare's managed TURN service and ships the
//! ready-made ICE-server list to clients inside the existing signaling
//! frames (`StartSignaling`, and `Identified` for mid-game rejoiners).
//!
//! Trust boundary: `TURN_KEY_ID` / `TURN_API_TOKEN` never leave this
//! process. Clients only ever see the minted per-match credentials, which
//! expire on their own after [`CREDENTIAL_TTL_SECS`].
//!
//! Fail-open by design: unset config or a failed Cloudflare call returns an
//! empty list (with a warn), and clients keep their built-in STUN-only
//! fallback — a Cloudflare outage degrades to today's behavior instead of
//! blocking match starts.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::Config;

/// One entry of a WebRTC `iceServers` list. Serialized verbatim into the
/// signaling frames (STUN entries carry no credentials, so those fields are
/// omitted rather than sent as null), and deserialized verbatim from
/// Cloudflare's `generate-ice-servers` response — same shape on purpose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// Credential lifetime. Must outlive the longest plausible match (plus a
/// rejoin late in it); one credential set is shared by a whole match, minted
/// at `/start`. Rejoiners get a fresh mint of their own, so this does not
/// need to cover the gap between start and rejoin either.
const CREDENTIAL_TTL_SECS: u64 = 4 * 60 * 60;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateResponse {
    ice_servers: Vec<IceServer>,
}

/// True when both TURN env vars are present. Split out so `main.rs` can warn
/// once at boot instead of per-match.
pub fn configured(cfg: &Config) -> bool {
    !cfg.turn_key_id.trim().is_empty() && !cfg.turn_api_token.trim().is_empty()
}

/// Mint a fresh short-lived STUN+TURN server list from Cloudflare. Returns an
/// empty list when TURN is unconfigured or the mint fails — never an error,
/// so callers can splice the result straight into a signaling frame.
pub async fn mint_ice_servers(cfg: &Config) -> Vec<IceServer> {
    if !configured(cfg) {
        return Vec::new();
    }

    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
        cfg.turn_key_id.trim()
    );

    // One process-wide client so repeated mints reuse its connection pool /
    // TLS session instead of paying setup each time. Building with only a
    // timeout cannot fail (reqwest::Client::new() unwraps the same builder
    // internally), so the expect is unreachable in practice.
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("default reqwest client construction cannot fail")
    });

    let response = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.turn_api_token.trim()))
        .json(&serde_json::json!({ "ttl": CREDENTIAL_TTL_SECS }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("turn: credential mint request failed: {e}");
            return Vec::new();
        }
    };

    if !response.status().is_success() {
        tracing::warn!("turn: cloudflare returned {}", response.status());
        return Vec::new();
    }

    match response.json::<GenerateResponse>().await {
        Ok(body) => {
            tracing::info!(
                "turn: minted credentials ({} ice server entries, ttl {}s)",
                body.ice_servers.len(),
                CREDENTIAL_TTL_SECS
            );
            body.ice_servers
        }
        Err(e) => {
            tracing::warn!("turn: malformed cloudflare response: {e}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented shape of Cloudflare's generate-ice-servers response
    /// must parse — STUN entry without credentials, TURN entry with them.
    #[test]
    fn parses_cloudflare_response() {
        let raw = r#"{
            "iceServers": [
                { "urls": ["stun:stun.cloudflare.com:3478", "stun:stun.cloudflare.com:53"] },
                {
                    "urls": [
                        "turn:turn.cloudflare.com:3478?transport=udp",
                        "turn:turn.cloudflare.com:3478?transport=tcp",
                        "turns:turn.cloudflare.com:5349?transport=tcp",
                        "turns:turn.cloudflare.com:443?transport=tcp"
                    ],
                    "username": "g0e2...",
                    "credential": "d0b8..."
                }
            ]
        }"#;
        let parsed: GenerateResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.ice_servers.len(), 2);
        assert!(parsed.ice_servers[0].username.is_none());
        assert_eq!(parsed.ice_servers[1].urls.len(), 4);
        assert_eq!(parsed.ice_servers[1].username.as_deref(), Some("g0e2..."));
        assert_eq!(parsed.ice_servers[1].credential.as_deref(), Some("d0b8..."));
    }

    /// STUN entries must serialize without null username/credential keys —
    /// clients treat key presence as "credentialed entry".
    #[test]
    fn serializes_without_null_credentials() {
        let stun = IceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".into()],
            username: None,
            credential: None,
        };
        let json = serde_json::to_string(&stun).unwrap();
        assert_eq!(json, r#"{"urls":["stun:stun.cloudflare.com:3478"]}"#);
    }

    #[test]
    fn unconfigured_when_either_var_blank() {
        let mut cfg = blank_config();
        assert!(!configured(&cfg));
        cfg.turn_key_id = "key".into();
        assert!(!configured(&cfg));
        cfg.turn_api_token = "  ".into();
        assert!(!configured(&cfg));
        cfg.turn_api_token = "token".into();
        assert!(configured(&cfg));
    }

    fn blank_config() -> Config {
        Config {
            redis_url: String::new(),
            game_port: 0,
            admin_port: 0,
            session_ttl_secs: 0,
            min_launcher_version: String::new(),
            min_game_version: String::new(),
            admin_password: String::new(),
            watchtower_url: String::new(),
            watchtower_token: String::new(),
            log_format: String::new(),
            turn_key_id: String::new(),
            turn_api_token: String::new(),
            admin_public_url: String::new(),
            oidc_issuer_url: String::new(),
            oidc_client_id: String::new(),
            oidc_client_secret: String::new(),
            break_glass_pepper: String::new(),
            oidc_group_prefix: String::new(),
        }
    }
}
