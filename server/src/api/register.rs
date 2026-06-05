use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use deadpool_redis::redis::AsyncCommands;
use rand::Rng;
use shared::protocol::messages::{RegisterRequest, RegisterResponse};
use shared::types::player::PlayerId;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, hash_token, FREELIST_KEY, PLAYER_COUNTER_KEY};

/// Cap username at 32 chars to match the launcher UI input; trimmed of
/// surrounding whitespace before storage so the admin Users tab sees
/// canonical values.
const MAX_USERNAME_LEN: usize = 32;

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_register.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let username = req.username.trim().to_string();
    if username.is_empty() || username.chars().count() > MAX_USERNAME_LEN {
        return Err(AppError::BadRequest("invalid username".into()));
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Try to reuse the prior identity. If anything about the lookup fails or
    // the token hash doesn't match, fall through to fresh issuance — the
    // launcher must be able to recover from a corrupted identity file.
    let reused = match (&req.prior_player_id, &req.prior_secret_token) {
        (Some(pid), Some(token)) if !pid.is_empty() && !token.is_empty() => {
            let stored: Option<String> = conn
                .get(format!("player:{}:token_hash", pid))
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            match stored {
                Some(h) if h == hash_token(token) => Some((pid.clone(), token.clone())),
                _ => None,
            }
        }
        _ => None,
    };

    let (player_id, secret_token) = match reused {
        Some(pair) => pair,
        None => {
            let number = allocate_player_number(&mut conn).await?;

            let pid = PlayerId::from_counter(number).to_string();
            let token_bytes: [u8; 32] = rand::thread_rng().gen();
            let token = hex::encode(token_bytes);
            let hash = hash_token(&token);

            conn.set::<_, _, ()>(format!("player:{}:token_hash", pid), &hash)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

            tracing::info!("registered player {}", pid);
            (pid, token)
        }
    };

    // Refresh username in Redis on every register — username can change at
    // any time on the launcher and the server is the canonical record.
    conn.set::<_, _, ()>(format!("player:{}:username", player_id), &username)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let dev_flag: bool = conn
        .get::<_, Option<String>>(format!("player:{}:dev_flag", player_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_deref()
        == Some("true");

    Ok(Json(RegisterResponse {
        player_id,
        secret_token,
        username,
        dev_flag,
    }))
}

/// Choose the next player-id number: reuse the **lowest** freed id from the
/// pool (`ZPOPMIN player:freelist`) if one is waiting, otherwise advance the
/// monotonic counter. `ZPOPMIN` removes the entry as it reads it and is atomic,
/// so two concurrent registrations can never be handed the same recycled
/// number. The counter is only touched when the pool is empty, so issued-id
/// totals still trend upward as deleted numbers get refilled lowest-first.
async fn allocate_player_number(conn: &mut deadpool_redis::Connection) -> Result<u64> {
    let popped: Vec<(String, f64)> = conn
        .zpopmin(FREELIST_KEY, 1)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Some(number) = freed_number_from_pop(&popped) {
        return Ok(number);
    }
    // Either the pool was empty or (defensively) held a malformed member that
    // we just consumed — fall through to a fresh high-water number rather than
    // failing the registration.
    if !popped.is_empty() {
        tracing::warn!(?popped, "discarding malformed freelist entry");
    }

    let counter: u64 = conn
        .incr(PLAYER_COUNTER_KEY, 1u64)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(counter)
}

/// Extract the reusable id number from a `ZPOPMIN` reply, accepting it only
/// when the popped member is a well-formed counter value. Pure + sync so the
/// pool-vs-counter decision is unit-testable without a live Redis.
fn freed_number_from_pop(popped: &[(String, f64)]) -> Option<u64> {
    popped
        .first()
        .and_then(|(member, _score)| member.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::freed_number_from_pop;

    #[test]
    fn empty_pop_yields_no_reuse() {
        // Empty pool → caller advances the counter instead.
        assert_eq!(freed_number_from_pop(&[]), None);
    }

    #[test]
    fn well_formed_pop_is_reused() {
        assert_eq!(freed_number_from_pop(&[("5".to_string(), 5.0)]), Some(5));
        // Tolerates a zero-padded member just in case one was ever stored that
        // way; parse() strips the leading zeros.
        assert_eq!(
            freed_number_from_pop(&[("000000042".to_string(), 42.0)]),
            Some(42)
        );
    }

    #[test]
    fn malformed_pop_falls_through() {
        // A non-numeric member is ignored so a corrupt pool entry can't crash
        // or poison a registration.
        assert_eq!(freed_number_from_pop(&[("not-a-number".to_string(), 1.0)]), None);
    }
}
