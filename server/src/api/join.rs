use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    Json,
};
use chrono::Utc;
use deadpool_redis::redis::Script;
use serde::Deserialize;
use shared::protocol::messages::{JoinRequest, JoinResponse, JoinedPeer};
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    state::AppState,
};
use super::{client_ip, validate_player, Session};

// Atomic join. Performs the entire read-modify-write inside Redis so two
// concurrent joiners can't both squeeze past a full capacity check.
//
// KEYS[1] = session key (e.g. "session:ABC123")
// ARGV[1] = joining player_id
// ARGV[2] = joined_at_ms (milliseconds since epoch, computed Rust-side)
// ARGV[3] = session TTL in seconds (for the SETEX after a successful append)
//
// Returns a JSON envelope discriminated on "result":
//   {"result": "ok", "session": <session>}
//   {"result": "not_found"}
//   {"result": "not_waiting"}
//   {"result": "is_host"}
//   {"result": "already_joined"}
//   {"result": "session_full", "capacity": <u8>}
const JOIN_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_waiting'})
end
if session.host_player_id == ARGV[1] then
  return cjson.encode({result = 'is_host'})
end
for _, j in ipairs(session.joiners) do
  if j.player_id == ARGV[1] then
    return cjson.encode({result = 'already_joined'})
  end
end
local current = 1 + #session.joiners
if current >= session.player_count then
  return cjson.encode({result = 'session_full', capacity = session.player_count})
end
table.insert(session.joiners, {player_id = ARGV[1], joined_at_ms = tonumber(ARGV[2])})
redis.call('SET', KEYS[1], cjson.encode(session), 'EX', tonumber(ARGV[3]))
return cjson.encode({result = 'ok', session = session})
"#;

#[derive(Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum JoinOutcome {
    Ok { session: Session },
    NotFound,
    NotWaiting,
    IsHost,
    AlreadyJoined,
    SessionFull { capacity: u8 },
}

pub async fn join(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<JoinRequest>,
) -> Result<Json<JoinResponse>> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_join.check_key(&ip).is_err() {
        tracing::warn!(client = %ip, "join rate-limited");
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let key = format!("session:{}", body.session_code);
    let joined_at_ms = Utc::now().timestamp_millis().max(0) as u64;

    let script = Script::new(JOIN_SCRIPT);
    let raw: String = script
        .key(&key)
        .arg(&body.player_id)
        .arg(joined_at_ms)
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let outcome: JoinOutcome = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(format!("join script returned malformed json: {e}")))?;

    let session = match outcome {
        JoinOutcome::Ok { session } => session,
        JoinOutcome::NotFound => return Err(AppError::NotFound),
        JoinOutcome::NotWaiting => {
            return Err(AppError::Conflict("session_already_active".to_string()))
        }
        JoinOutcome::IsHost => {
            return Err(AppError::Conflict("cannot_join_own_session".to_string()))
        }
        JoinOutcome::AlreadyJoined => {
            return Err(AppError::Conflict("already_joined".to_string()))
        }
        JoinOutcome::SessionFull { capacity } => {
            return Err(AppError::SessionFull { capacity })
        }
    };

    let current_player_count = session.current_player_count();
    let joiners: Vec<JoinedPeer> = session
        .joiners
        .iter()
        .map(|j| JoinedPeer { player_id: j.player_id.clone() })
        .collect();

    tracing::info!(
        "player {} joined session {} ({}/{} players)",
        body.player_id, body.session_code, current_player_count, session.player_count
    );

    Ok(Json(JoinResponse {
        gamemode: session.gamemode,
        win_condition: session.win_condition,
        spawn_settings: session.spawn_settings,
        player_count: session.player_count,
        current_player_count,
        joiners,
    }))
}
