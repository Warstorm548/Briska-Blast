use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use deadpool_redis::redis::{AsyncCommands, Script};
use serde::Deserialize;
use shared::protocol::messages::StartSessionRequest;
use std::collections::HashSet;
use std::net::SocketAddr;

use crate::{
    error::{AppError, Result},
    gamemode,
    signaling::ServerMsg,
    state::AppState,
};
use super::{client_ip, validate_player, Session};

// Atomic transition Waiting → Starting. Equivalent to a Redis
// WATCH/MULTI/EXEC CAS, but expressed as a single Lua script so the
// whole read-validate-write happens in one Redis round-trip with no
// other commands interleaving — same atomicity guarantee as the join
// handler.
//
// We pass `expected_joiner_count` so the script can detect a concurrent
// /join that landed between our Rust-side WS-ready check and the
// transaction. On mismatch the script returns `conflict` and Rust
// retries the whole flow (re-reading the session and re-running the
// WS-ready check against the fresh state).
//
// KEYS[1] = session key
// ARGV[1] = expected host player_id
// ARGV[2] = gamemode min_players (host included)
// ARGV[3] = expected joiner count (len of session.joiners at WS-ready time)
// ARGV[4] = session TTL in seconds
const START_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.host_player_id ~= ARGV[1] then
  return cjson.encode({result = 'not_host'})
end
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_in_waiting'})
end
local current = 1 + #session.joiners
if current < tonumber(ARGV[2]) then
  return cjson.encode({result = 'below_min_players'})
end
if #session.joiners ~= tonumber(ARGV[3]) then
  return cjson.encode({result = 'conflict'})
end
session.status = 'starting'
-- Freeze the seating roster ([host, ...joiners] in join order) at the instant
-- of Start. Nothing mutates session.seat_order afterwards, so a later host
-- promotion (which reorders joiners/host_player_id) leaves the original seating
-- intact for every client, including a rejoiner. Mirrors `session_members` in
-- the Rust StartSignaling broadcast.
local seat_order = {session.host_player_id}
for i = 1, #session.joiners do
  seat_order[#seat_order + 1] = session.joiners[i].player_id
end
session.seat_order = seat_order
redis.call('SET', KEYS[1], cjson.encode(session), 'EX', tonumber(ARGV[4]))
return cjson.encode({result = 'ok'})
"#;

#[derive(Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum StartOutcome {
    Ok,
    NotFound,
    NotHost,
    NotInWaiting,
    BelowMinPlayers,
    Conflict,
}

const MAX_START_RETRIES: u32 = 3;

/// `POST /session/:code/start` — host-only `Waiting` → `Starting` transition. Runs
/// a Lua CAS (WS-ready check + optimistic retry) that also freezes the seating
/// roster, then broadcasts `StartSignaling` so every client begins WebRTC setup.
pub async fn start_session(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<StartSessionRequest>,
) -> Result<StatusCode> {
    let ip = client_ip(&ConnectInfo(addr), &headers);
    if state.rl_session.check_key(&ip).is_err() {
        return Err(AppError::TooManyRequests);
    }

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    validate_player(&mut conn, &body.player_id, &body.secret_token).await?;

    let key = format!("session:{}", code);

    // Optimistic retry loop. The Lua script rejects the transition on
    // joiner-count mismatch (a concurrent /join landed); we re-read and
    // re-check WS-readiness against the new snapshot.
    for _attempt in 0..MAX_START_RETRIES {
        // Read the current snapshot. Used for the WS-ready check (which
        // can't be expressed inside Lua — SignalHub lives in-process).
        let raw: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let raw = raw.ok_or(AppError::NotFound)?;
        let session: Session = serde_json::from_str(&raw)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        // WS-ready check uses this snapshot's member list. The Lua's
        // expected-joiner-count guard is what protects us from a join
        // landing between this check and the commit.
        let ws_members: HashSet<String> = state
            .signal_hub
            .room_members(&code)
            .await
            .into_iter()
            .collect();
        let session_members: Vec<String> = std::iter::once(session.host_player_id.clone())
            .chain(session.joiners.iter().map(|j| j.player_id.clone()))
            .collect();
        if !session_members.iter().all(|p| ws_members.contains(p)) {
            return Err(AppError::SessionNotStartable {
                reason: "not_all_peers_ready",
            });
        }

        let (min_players, _max) = gamemode::bounds_for(session.gamemode);
        let expected_joiner_count = session.joiners.len();

        let script = Script::new(START_SCRIPT);
        let result_json: String = script
            .key(&key)
            .arg(&body.player_id)
            .arg(min_players)
            .arg(expected_joiner_count)
            .arg(state.config.session_ttl_secs)
            .invoke_async(&mut *conn)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let outcome: StartOutcome = serde_json::from_str(&result_json)
            .map_err(|e| AppError::Internal(format!("malformed start lua result: {e}")))?;

        match outcome {
            StartOutcome::Ok => {
                // Seed the in-memory room with the win target so the scoring path
                // can detect game over. Every member is WS-ready here (checked
                // above), so the room exists. Lost on a server restart along with
                // the in-memory tally, same as the score state itself.
                state
                    .signal_hub
                    .set_win_target(&code, session.win_condition.target() as i64)
                    .await;

                // One credential set shared by the whole match (TTL outlives
                // it — see turn.rs); empty on mint failure or when TURN is
                // unconfigured, in which case clients keep their built-in
                // STUN-only fallback rather than the start being blocked.
                let ice_servers = crate::turn::mint_ice_servers(&state.config).await;

                // Best-effort broadcast. Clients that miss it can re-poll
                // /session/:code to discover the new Starting status.
                state
                    .signal_hub
                    .broadcast(
                        &code,
                        ServerMsg::StartSignaling {
                            gamemode: session.gamemode,
                            win_condition: session.win_condition,
                            spawn_settings: session.spawn_settings,
                            player_count: session.player_count,
                            peers: session_members,
                            ice_servers,
                        },
                        None,
                    )
                    .await;
                tracing::info!(
                    "player {} started session {} ({}/{} players)",
                    body.player_id,
                    code,
                    session.current_player_count(),
                    session.player_count
                );
                return Ok(StatusCode::NO_CONTENT);
            }
            StartOutcome::NotFound => return Err(AppError::NotFound),
            StartOutcome::NotHost => {
                return Err(AppError::SessionNotStartable { reason: "not_host" })
            }
            StartOutcome::NotInWaiting => {
                return Err(AppError::SessionNotStartable { reason: "not_in_waiting" })
            }
            StartOutcome::BelowMinPlayers => {
                return Err(AppError::SessionNotStartable { reason: "below_min_players" })
            }
            StartOutcome::Conflict => {
                // A concurrent /join changed the joiner list between our
                // WS-ready check and the transaction. Re-read and retry.
                tracing::debug!("start: conflict on session {}, retrying", code);
                continue;
            }
        }
    }

    tracing::warn!(
        "start: exhausted {} retries on session {} — concurrent /join activity",
        MAX_START_RETRIES, code
    );
    Err(AppError::SessionNotStartable { reason: "conflict" })
}
