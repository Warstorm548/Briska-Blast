use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use deadpool_redis::redis::{AsyncCommands, Script};
use serde::Deserialize;
use shared::protocol::messages::{
    CloseSessionRequest, SessionPollResponse, TransferHostRequest,
};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    signaling::ServerMsg,
    state::AppState,
};
use super::{client_ip, validate_player, Session};

pub async fn get_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<SessionPollResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        state.warn_rate_limited(&ip, "session request");
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw: Option<String> = conn
        .get(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw = raw.ok_or(AppError::NotFound)?;

    let session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let current_player_count = session.current_player_count();
    let joiner_player_ids = session
        .joiners
        .iter()
        .map(|j| j.player_id.clone())
        .collect();

    Ok(Json(SessionPollResponse {
        status: session.status,
        gamemode: session.gamemode,
        win_condition: session.win_condition,
        spawn_settings: session.spawn_settings,
        player_count: session.player_count,
        current_player_count,
        joiner_player_ids,
    }))
}

pub async fn close_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<CloseSessionRequest>,
) -> Result<StatusCode> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        state.warn_rate_limited(&ip, "session request");
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let raw: Option<String> = conn
        .get(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let raw = raw.ok_or(AppError::NotFound)?;

    let session: Session = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if session.host_player_id != body.player_id {
        return Err(AppError::Unauthorized);
    }

    conn.del::<_, ()>(format!("session:{}", code))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!("player {} closed session {}", body.player_id, code);

    Ok(StatusCode::NO_CONTENT)
}

// Atomic host handoff. Same single-Lua-script atomicity as the join/start
// handlers — the whole read-validate-swap-write runs without any other
// command interleaving. Restricted to Waiting: a transfer mid-game is a
// different concern (handled by the deferred auto-promotion work).
//
// The demoted host re-enters `joiners` with a fresh `joined_at_ms`, so they
// sit at the back of the chronological join order. That order only matters
// for future auto-promotion; a voluntary handoff putting the old host last
// is the intuitive choice.
//
// KEYS[1] = session key
// ARGV[1] = current host player_id (the caller)
// ARGV[2] = new host player_id (must be a current joiner)
// ARGV[3] = now in ms (timestamp for the demoted host's joiner entry)
// ARGV[4] = session TTL in seconds
const TRANSFER_HOST_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.host_player_id ~= ARGV[1] then
  return cjson.encode({result = 'not_host'})
end
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_waiting'})
end
local found_idx = nil
for i, j in ipairs(session.joiners) do
  if j.player_id == ARGV[2] then
    found_idx = i
    break
  end
end
if not found_idx then
  return cjson.encode({result = 'target_not_joiner'})
end
table.remove(session.joiners, found_idx)
table.insert(session.joiners, {player_id = ARGV[1], joined_at_ms = tonumber(ARGV[3])})
session.host_player_id = ARGV[2]
redis.call('SET', KEYS[1], cjson.encode(session), 'EX', tonumber(ARGV[4]))
return cjson.encode({result = 'ok'})
"#;

#[derive(Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum TransferOutcome {
    Ok,
    NotFound,
    NotHost,
    NotWaiting,
    TargetNotJoiner,
}

pub async fn transfer_host(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<TransferHostRequest>,
) -> Result<StatusCode> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        state.warn_rate_limited(&ip, "session request");
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let key = format!("session:{}", code);
    let now_ms = Utc::now().timestamp_millis().max(0) as u64;

    let script = Script::new(TRANSFER_HOST_SCRIPT);
    let raw: String = script
        .key(&key)
        .arg(&body.player_id)
        .arg(&body.new_host_player_id)
        .arg(now_ms)
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let outcome: TransferOutcome = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(format!("malformed transfer lua result: {e}")))?;

    match outcome {
        TransferOutcome::Ok => {
            state
                .signal_hub
                .broadcast(
                    &code,
                    ServerMsg::HostChanged {
                        player_id: body.new_host_player_id.clone(),
                    },
                    None,
                )
                .await;
            tracing::info!(
                "host of session {} transferred from {} to {}",
                code, body.player_id, body.new_host_player_id
            );
            Ok(StatusCode::NO_CONTENT)
        }
        TransferOutcome::NotFound => Err(AppError::NotFound),
        TransferOutcome::NotHost => Err(AppError::Unauthorized),
        TransferOutcome::NotWaiting => {
            Err(AppError::Conflict("session_already_active".to_string()))
        }
        TransferOutcome::TargetNotJoiner => {
            Err(AppError::BadRequest("target_not_joiner".to_string()))
        }
    }
}
