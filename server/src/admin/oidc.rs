//! Pocket ID OIDC login for the admin panel — Authorization Code flow with
//! PKCE (S256) against a confidential client.
//!
//! The flow, end to end:
//!   1. `GET /admin/oidc/login` — mint `state`/`nonce`/PKCE `code_verifier`,
//!      stash the verifier+nonce in Redis keyed by `state` (one-time, 15-min
//!      TTL), and 302 the browser to Pocket ID's authorize endpoint.
//!   2. The user taps their passkey; Pocket ID redirects back to
//!      `GET /admin/oidc/callback?code=…&state=…`.
//!   3. We consume the stashed flow (unknown `state` ⇒ CSRF/expired ⇒ reject),
//!      exchange the code for tokens (client-secret-basic + `code_verifier`),
//!      validate the `id_token` against the JWKS (signature, `iss`, `aud`,
//!      `exp`, `nonce`), map its `groups` claim to an [`AdminRole`], and mint
//!      the existing admin session cookie.
//!
//! Trust boundary: `OIDC_CLIENT_SECRET` never leaves this process. Login
//! authorizes straight from the validated `id_token`; we do not keep the OIDC
//! refresh token — the local Redis session (idle TTL, keepalive, logout) takes
//! over once a person is in. Fail-open: when OIDC isn't configured these
//! handlers are unreachable in practice (the button/routes only exist when
//! `oidc_enabled()`), and they defensively bounce to `/admin`.

use axum::{
    extract::{Query, State},
    http::HeaderValue,
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use deadpool_redis::redis::AsyncCommands;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, DecodingKey, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::OnceCell;

use super::{create_session, role_from_groups, set_cookie, templates};
use crate::state::AppState;

/// Login-flow record lifetime — matches Pocket ID's 15-minute auth-code window.
const FLOW_TTL_SECS: u64 = 900;

/// Process-wide client for token/JWKS/discovery calls (reuses its connection
/// pool). Building with only a timeout cannot fail.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .expect("default reqwest client construction cannot fail")
    })
}

/// Separate short-timeout client for the login-page health probe, so a hung
/// Pocket ID can't stall page rendering for the full 8s.
fn health_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("default reqwest client construction cannot fail")
    })
}

/// The subset of Pocket ID's discovery document we use. The issuer is fixed
/// per deployment, so one process-wide cache is correct.
#[derive(Clone, Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

static DISCOVERY: OnceCell<Discovery> = OnceCell::const_new();

/// Fetch + cache the discovery document. A failed fetch is not cached
/// (`OnceCell` only stores on `Ok`), so a later login attempt retries.
async fn get_discovery(issuer: &str) -> Option<&'static Discovery> {
    DISCOVERY
        .get_or_try_init(|| async {
            let url = format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            );
            let doc = client()
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .json::<Discovery>()
                .await?;
            Ok::<Discovery, reqwest::Error>(doc)
        })
        .await
        .ok()
}

/// Live check the login page uses to decide whether to reveal the emergency
/// password form. A fresh short-timeout GET — a cached discovery doc is not
/// proof Pocket ID is reachable *right now*.
pub async fn pocket_id_healthy(issuer: &str) -> bool {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    matches!(
        health_client().get(&url).send().await,
        Ok(resp) if resp.status().is_success()
    )
}

/// Stashed in Redis under `oidc:flow:{state}` between the authorize redirect
/// and the callback.
#[derive(Serialize, Deserialize)]
struct FlowRecord {
    verifier: String,
    nonce: String,
}

/// A URL-safe random token (43 base64url chars ≈ 256 bits) — used for `state`,
/// `nonce`, and the PKCE `code_verifier` (all valid unreserved-char strings).
fn random_token() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE S256 challenge: base64url(SHA256(ASCII(verifier))).
fn code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn notice(title: &str, message: &str) -> Response {
    Html(templates::notice_page(title, message)).into_response()
}

/// GET /admin/oidc/login — start the Authorization Code + PKCE flow.
pub async fn oidc_login(State(state): State<AppState>) -> Response {
    if !state.config.oidc_enabled() {
        return Redirect::to("/admin").into_response();
    }
    let Some(disc) = get_discovery(&state.config.oidc_issuer_url).await else {
        return notice(
            "Pocket ID unavailable",
            "Could not reach Pocket ID to start sign-in. Try again, or use the emergency login.",
        );
    };

    let state_tok = random_token();
    let nonce = random_token();
    let verifier = random_token();
    let challenge = code_challenge(&verifier);

    let flow = FlowRecord {
        verifier,
        nonce: nonce.clone(),
    };
    let Ok(flow_json) = serde_json::to_string(&flow) else {
        return notice("Sign-in error", "Could not start sign-in. Try again.");
    };
    let Ok(mut conn) = state.redis.get().await else {
        return notice("Sign-in error", "Server error starting sign-in. Try again.");
    };
    if conn
        .set_ex::<_, _, ()>(format!("oidc:flow:{state_tok}"), flow_json, FLOW_TTL_SECS)
        .await
        .is_err()
    {
        return notice("Sign-in error", "Server error starting sign-in. Try again.");
    }

    let redirect_uri = state.config.oidc_redirect_uri();
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        disc.authorization_endpoint,
        enc(&state.config.oidc_client_id),
        enc(&redirect_uri),
        enc("openid profile email groups"),
        state_tok,
        nonce,
        challenge,
    );
    Redirect::to(&url).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// GET /admin/oidc/callback — finish the flow and mint the admin session.
pub async fn oidc_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if !state.config.oidc_enabled() {
        return Redirect::to("/admin").into_response();
    }
    if let Some(err) = q.error {
        return notice("Sign-in cancelled", &format!("Pocket ID reported: {err}"));
    }
    let (Some(code), Some(state_tok)) = (q.code, q.state) else {
        return notice(
            "Sign-in error",
            "Missing authorization code. Please try signing in again.",
        );
    };

    // Recover + consume the flow (one-time use). An unknown `state` is either a
    // CSRF attempt or an expired attempt — both rejected the same way.
    let Ok(mut conn) = state.redis.get().await else {
        return notice("Sign-in error", "Server error. Try again.");
    };
    let flow_key = format!("oidc:flow:{state_tok}");
    let flow_json: Option<String> = conn.get(&flow_key).await.unwrap_or(None);
    let _: () = conn.del(&flow_key).await.unwrap_or(());
    let Some(flow) = flow_json.and_then(|j| serde_json::from_str::<FlowRecord>(&j).ok()) else {
        return notice(
            "Sign-in expired",
            "Your sign-in attempt expired or was invalid. Please try again.",
        );
    };

    let Some(disc) = get_discovery(&state.config.oidc_issuer_url).await else {
        return notice(
            "Pocket ID unavailable",
            "Could not reach Pocket ID to finish sign-in. Try again.",
        );
    };

    let id_token = match exchange_code(disc, &state, &code, &flow.verifier).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("oidc: token exchange failed: {e}");
            return notice(
                "Sign-in failed",
                "Could not complete sign-in with Pocket ID. Try again.",
            );
        }
    };

    let claims = match validate_id_token(disc, &state, &id_token, &flow.nonce).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("oidc: id_token validation failed: {e}");
            return notice(
                "Sign-in failed",
                "Could not verify your Pocket ID identity. Try again.",
            );
        }
    };

    let Some(role) = role_from_groups(&claims.groups, &state.config.oidc_group_names()) else {
        tracing::info!(sub = %claims.sub, "oidc: user in no briska admin group — denied");
        return notice(
            "Not authorized",
            "Your account isn't a member of a Briska admin group. Ask a SuperAdmin for access.",
        );
    };

    let username = claims
        .preferred_username
        .or(claims.name)
        .or(claims.email)
        .unwrap_or_else(|| claims.sub.clone());

    let Some(token) = create_session(&state.redis, role, &username, &claims.sub).await else {
        return notice("Sign-in error", "Could not create your session. Try again.");
    };

    tracing::info!(user = %username, role = role.label(), "oidc: admin signed in");
    let mut response = Redirect::to("/admin/dashboard").into_response();
    response.headers_mut().insert(
        "Set-Cookie",
        HeaderValue::from_str(&set_cookie(&token)).unwrap(),
    );
    response
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// Exchange the authorization code for tokens. Confidential client: we
/// authenticate with client-secret-basic AND send the PKCE `code_verifier`.
async fn exchange_code(
    disc: &Discovery,
    state: &AppState,
    code: &str,
    verifier: &str,
) -> Result<String, String> {
    let redirect_uri = state.config.oidc_redirect_uri();
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", state.config.oidc_client_id.as_str()),
        ("code_verifier", verifier),
    ];
    let resp = client()
        .post(&disc.token_endpoint)
        .basic_auth(
            &state.config.oidc_client_id,
            Some(&state.config.oidc_client_secret),
        )
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("token endpoint returned {}", resp.status()));
    }
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("bad token response: {e}"))?;
    Ok(body.id_token)
}

#[derive(Deserialize)]
struct IdClaims {
    sub: String,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

/// Fully validate the `id_token`: signature against the JWKS key matching the
/// header `kid`, plus `iss` / `aud` / `exp` (jsonwebtoken) and `nonce` (here,
/// since the library doesn't check it).
async fn validate_id_token(
    disc: &Discovery,
    state: &AppState,
    id_token: &str,
    expected_nonce: &str,
) -> Result<IdClaims, String> {
    let header = decode_header(id_token).map_err(|e| format!("bad header: {e}"))?;
    let kid = header.kid.ok_or("id_token missing kid")?;

    let jwks: JwkSet = client()
        .get(&disc.jwks_uri)
        .send()
        .await
        .map_err(|e| format!("jwks fetch error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("bad jwks: {e}"))?;
    let jwk = jwks.find(&kid).ok_or("no jwk matches id_token kid")?;
    let key = DecodingKey::from_jwk(jwk).map_err(|e| format!("bad jwk: {e}"))?;

    let mut validation = Validation::new(header.alg);
    validation.set_audience(&[state.config.oidc_client_id.as_str()]);
    validation.set_issuer(&[state.config.oidc_issuer_url.trim_end_matches('/')]);

    let data =
        decode::<IdClaims>(id_token, &key, &validation).map_err(|e| format!("verify error: {e}"))?;

    match &data.claims.nonce {
        Some(n) if n == expected_nonce => Ok(data.claims),
        _ => Err("nonce mismatch".into()),
    }
}
