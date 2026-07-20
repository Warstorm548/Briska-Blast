//! Atomic session-state mutations on host loss or joiner leave. Each is a single
//! Redis Lua script so the read → classify → write/delete happens as one
//! operation that can't race a concurrent `/session/:code/start` transition.
//! Used by the disconnect cleanup in [`super`] and [`super::disconnect`].

use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;

use crate::{signaling::protocol::ServerMsg, state::AppState};

/// How long the ready barrier waits for every seated player's `ClientReady`
/// before starting the match anyway. Sits under the clients' own 30s Preparing
/// deadline so a slow-but-alive lobby always resolves server-side first.
pub(crate) const READY_GRACE_SECS: u64 = 20;

/// How long a pause-on-rejoin may hold the match frozen before the valve
/// resumes it anyway (the rejoiner's edge then stays walled until its mesh
/// heals). Sits under the rejoiner's own 30s Preparing deadline, so a rejoin
/// that will ultimately fail can't freeze everyone for its full window.
pub(super) const PAUSE_VALVE_SECS: u64 = 25;

/// The shared unfreeze countdown clients run when a pause ends.
pub(super) const RESUME_COUNTDOWN_SECS: u64 = 3;

/// Release `player_id`'s pause hold and broadcast `MatchResumed` iff that
/// emptied the pause set. The one resume funnel — the rejoiner's
/// `ClientReady`, its disconnect cleanup, and the valve all call this, and
/// `clear_pause`'s remove-wins semantics make the broadcast single-shot.
pub(super) async fn resume_if_cleared(state: &AppState, code: &str, player_id: &str) {
    if state.signal_hub.clear_pause(code, player_id).await {
        state
            .signal_hub
            .broadcast(
                code,
                ServerMsg::MatchResumed { countdown_secs: RESUME_COUNTDOWN_SECS },
                None,
            )
            .await;
        tracing::info!("ws: session {} resuming ({}'s pause hold cleared)", code, player_id);
    }
}

/// Arm the pause valve when a rejoiner pauses the match: after
/// [`PAUSE_VALVE_SECS`], release its hold whether or not it finished
/// re-meshing (same spawned-timer shape as the ready barrier's valve;
/// `clear_pause` itself is the single-winner latch, so a rejoiner that
/// already readied up or dropped makes this a no-op).
pub(super) fn spawn_pause_valve(state: AppState, code: String, player_id: String) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(PAUSE_VALVE_SECS)).await;
        resume_if_cleared(&state, &code, &player_id).await;
    });
}

/// Resolve the ready barrier: tell the room the match is on and flip the
/// session `starting → active`. Called by exactly one winner of the
/// `match_started` latch — either the last `ClientReady` (`AllReady`) or the
/// grace valve (`force_match_start`) — so it runs once per match.
///
/// Ordering: broadcast first, then the Redis flip. The broadcast is what
/// unblocks clients; the status flip only affects later observers (GET
/// /session polls, rejoin classification), so its latency shouldn't sit in
/// front of the start signal.
pub(crate) async fn start_match(state: &AppState, code: &str) {
    state
        .signal_hub
        .broadcast(code, ServerMsg::MatchStarted {}, None)
        .await;
    activate_session(state, code).await;
}

/// Arm the barrier's grace valve at `/start`: after [`READY_GRACE_SECS`],
/// start the match for whoever is ready. The `match_started` latch (not this
/// timer) is the single-winner mechanism — if every player readied up first,
/// `force_match_start` reports the barrier already resolved and the valve
/// exits without acting. Deliberately NOT the `arm_grace` map: that refuses to
/// arm while the keyed player has a live socket, which everyone here does.
pub(crate) fn spawn_ready_barrier(state: AppState, code: String) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(READY_GRACE_SECS)).await;
        if state.signal_hub.force_match_start(&code).await {
            tracing::info!(
                "ws: session {} ready barrier timed out after {}s — starting anyway",
                code, READY_GRACE_SECS
            );
            start_match(&state, &code).await;
        }
    });
}

/// Atomic `starting → active` status flip, the Redis half of the barrier
/// resolving. A Lua CAS modeled on `/start`'s `START_SCRIPT`: the read,
/// status check, and write happen as one Redis operation, so a concurrent
/// teardown (session deleted) or a duplicate activation can't interleave —
/// the loser just logs at debug. Losing is expected in benign races; the
/// in-memory `match_started` latch already made the start itself single-shot.
async fn activate_session(state: &AppState, code: &str) {
    // lua-cjson re-encode caveat: an empty `session.joiners` would come back as
    // `{}` (see remove_joiner_on_leave). It can't actually be empty here — a
    // Starting session froze ≥2 seats and a fully-emptied roster deletes the
    // session — but the gsub guard keeps this script safe if that ever shifts.
    const ACTIVATE_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'gone'})
end
local session = cjson.decode(raw)
if session.status ~= 'starting' then
  return cjson.encode({result = 'not_starting'})
end
session.status = 'active'
local encoded = cjson.encode(session)
if #session.joiners == 0 then
  encoded = string.gsub(encoded, '"joiners":{}', '"joiners":[]')
end
redis.call('SET', KEYS[1], encoded, 'EX', tonumber(ARGV[1]))
return cjson.encode({result = 'activated'})
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum ActivateOutcome {
        Activated,
        Gone,
        NotStarting,
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("ws: activate_session could not reach Redis for {}: {}", code, e);
            return;
        }
    };
    let script = deadpool_redis::redis::Script::new(ACTIVATE_SCRIPT);
    let raw: Result<String, _> = script
        .key(format!("session:{}", code))
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await;
    let outcome = raw
        .map_err(|e| e.to_string())
        .and_then(|r| serde_json::from_str::<ActivateOutcome>(&r).map_err(|e| e.to_string()));

    match outcome {
        Ok(ActivateOutcome::Activated) => {
            tracing::info!("ws: session {} is now active (ready barrier resolved)", code);
        }
        Ok(ActivateOutcome::Gone) | Ok(ActivateOutcome::NotStarting) => {
            tracing::debug!("ws: activate_session for {} was a no-op", code);
        }
        Err(e) => tracing::warn!("ws: activate_session failed for {}: {}", code, e),
    }
}

/// The single, shared "this session is over" teardown: delete the session from
/// Redis (idempotent) and broadcast `SessionEnded { reason }` to the room. The
/// win path calls this with `reason = "game_over"` right after its `GameOver` UI
/// signal — it performs the same DEL-then-`SessionEnded` shape the host-loss and
/// last-player-left branches do below, so the win path reuses the one cleanup
/// concept rather than introducing a parallel teardown. A later WS-close then
/// finds the session gone and no-ops, so no spurious host promotion fires.
///
/// Unlike `end_session_if_waiting` / `remove_joiner_on_leave`, this needs no
/// atomic status-checked Lua: the match is already active and decided over, so
/// there's no concurrent `/start` transition to race.
pub(super) async fn end_session(state: &AppState, code: &str, reason: &'static str) {
    match state.redis.get().await {
        Ok(mut conn) => {
            if let Err(e) = conn.del::<_, ()>(format!("session:{}", code)).await {
                tracing::warn!("ws: end_session DEL failed for {}: {}", code, e);
            }
        }
        Err(e) => tracing::warn!("ws: end_session could not reach Redis for {}: {}", code, e),
    }
    state
        .signal_hub
        .broadcast(code, ServerMsg::SessionEnded { reason }, None)
        .await;
    tracing::info!("ws: session {} ended ({})", code, reason);
}

/// What the host's disconnect cleanup found, so the caller knows whether to
/// promote. `end_session_if_waiting` both *acts* (ends the lobby if Waiting)
/// and *classifies* (reports when the match is live and needs promotion).
pub(super) enum HostDisconnectStage {
    /// Session was still Waiting; it has been deleted and `SessionEnded`
    /// broadcast. Nothing more for the caller to do.
    EndedWaiting,
    /// Session no longer exists (already gone). Nothing to do.
    SessionGone,
    /// Session is past Waiting (a live match). The caller promotes the next
    /// player or arms the reconnect grace window.
    Active,
}

/// On host disconnect: if the session is still in Waiting, end it and notify
/// peers (joiners can't start without a host) and return `EndedWaiting`. Past
/// Waiting, leave the session intact and return `Active` so the caller can
/// promote / arm grace.
///
/// Atomicity matters here: a non-atomic GET-then-DEL races with
/// `/session/:code/start` transitioning Waiting → Starting between the
/// two calls. A single Lua script performs the read, status check, and
/// conditional DEL as one Redis operation — no other command can
/// interleave.
pub(super) async fn end_session_if_waiting(state: &AppState, code: &str) -> Result<HostDisconnectStage, String> {
    const END_IF_WAITING_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
if session.status ~= 'waiting' then
  return cjson.encode({result = 'not_waiting'})
end
redis.call('DEL', KEYS[1])
return cjson.encode({result = 'deleted'})
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum EndOutcome {
        Deleted,
        NotFound,
        NotWaiting,
    }

    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(END_IF_WAITING_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: EndOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed lua result: {e}"))?;

    match outcome {
        EndOutcome::NotFound => Ok(HostDisconnectStage::SessionGone),
        // Advanced past Waiting (likely a concurrent /start): the match is live.
        EndOutcome::NotWaiting => Ok(HostDisconnectStage::Active),
        EndOutcome::Deleted => {
            state
                .signal_hub
                .broadcast(
                    code,
                    ServerMsg::SessionEnded { reason: "host_disconnect" },
                    None,
                )
                .await;
            tracing::info!("ws: session {} ended (host disconnected during waiting)", code);
            Ok(HostDisconnectStage::EndedWaiting)
        }
    }
}

/// Promote the next player after a host is lost from a live (past-Waiting)
/// session, or end the session if too few players remain to continue. Picks the
/// **oldest still-connected joiner** — chronological join order, skipping any
/// joiner whose WS is gone — and requires at least two connected players to
/// keep playing (a lone survivor has no one to play, matching the design's
/// "1 player remains → session ends"). Atomic single Lua script, guarded on the
/// departing player still being the host so a double disconnect can't promote
/// twice.
///
/// When `keep_ex_host` is true (a transient host drop), the departing host is
/// **demoted into `joiners`** at the back of the join order so they remain a
/// member and can rejoin as a non-host within their reconnect window. When false
/// (a deliberate host `Leave`), they're dropped. In the promoted branch
/// `joiners` is always non-empty (promotion needs ≥2 connected joiners, so ≥1
/// remains after removing the promoted one), so the empty-table re-encode quirk
/// can't arise.
pub(super) async fn promote_demote_or_end_active(
    state: &AppState,
    code: &str,
    departing_host: &str,
    keep_ex_host: bool,
) -> Result<(), String> {
    // KEYS[1] = session key
    // ARGV[1] = departing host player_id (guard)
    // ARGV[2] = JSON array of currently-connected player_ids
    // ARGV[3] = session TTL seconds
    // ARGV[4] = now in ms (joined_at_ms for the demoted ex-host)
    // ARGV[5] = "1" to demote-and-keep the ex-host, "0" to drop them
    const PROMOTE_OR_END_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'gone'})
end
local session = cjson.decode(raw)
if session.status == 'waiting' then
  return cjson.encode({result = 'not_active'})
end
if session.host_player_id ~= ARGV[1] then
  return cjson.encode({result = 'not_host'})
end
local connected = {}
for _, pid in ipairs(cjson.decode(ARGV[2])) do
  connected[pid] = true
end
local promote_idx = nil
local connected_count = 0
for i, j in ipairs(session.joiners) do
  if connected[j.player_id] then
    connected_count = connected_count + 1
    if promote_idx == nil then
      promote_idx = i
    end
  end
end
if connected_count >= 2 then
  local new_host = session.joiners[promote_idx].player_id
  table.remove(session.joiners, promote_idx)
  session.host_player_id = new_host
  if ARGV[5] == '1' then
    -- Demote the ex-host to the back of the join order (kept for reconnect).
    table.insert(session.joiners, {player_id = ARGV[1], joined_at_ms = tonumber(ARGV[4])})
  end
  redis.call('SET', KEYS[1], cjson.encode(session), 'EX', tonumber(ARGV[3]))
  return cjson.encode({result = 'promoted', new_host = new_host})
else
  redis.call('DEL', KEYS[1])
  return cjson.encode({result = 'ended'})
end
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum PromoteOutcome {
        Promoted { new_host: String },
        Ended,
        Gone,
        NotActive,
        NotHost,
    }

    // Live members at promotion time. The departing host's sender was already
    // removed by leave_room before this runs, so it won't appear here; filter
    // anyway to be defensive.
    let connected: Vec<String> = state
        .signal_hub
        .room_members(code)
        .await
        .into_iter()
        .filter(|p| p != departing_host)
        .collect();
    let connected_json = serde_json::to_string(&connected).map_err(|e| e.to_string())?;

    let now_ms = Utc::now().timestamp_millis().max(0) as u64;
    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(PROMOTE_OR_END_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .arg(departing_host)
        .arg(connected_json)
        .arg(state.config.session_ttl_secs)
        .arg(now_ms)
        .arg(if keep_ex_host { "1" } else { "0" })
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: PromoteOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed promote lua result: {e}"))?;

    match outcome {
        PromoteOutcome::Promoted { new_host } => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::HostChanged { player_id: new_host.clone() }, None)
                .await;
            tracing::info!("ws: session {} promoted {} to host after host loss", code, new_host);
        }
        PromoteOutcome::Ended => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::SessionEnded { reason: "host_disconnect" }, None)
                .await;
            tracing::info!("ws: session {} ended after host loss (too few players remain)", code);
        }
        // Already handled by another path (concurrent promotion, the session
        // was torn down elsewhere, or it was never active). No broadcast.
        PromoteOutcome::Gone | PromoteOutcome::NotActive | PromoteOutcome::NotHost => {
            tracing::debug!("ws: promote_demote_or_end_active for {} was a no-op", code);
        }
    }
    Ok(())
}

/// On an explicit joiner leave, remove that joiner from the session's Redis
/// roster. In Waiting this frees a lobby slot (so capacity / `/start`'s
/// all-peers-ready check stay correct); past Waiting it keeps GET /session
/// honest and, if the leave empties the roster (only the host remains), ends
/// the now-unplayable match. A transient socket drop never reaches here — it
/// keeps the slot for reconnect.
///
/// Atomic for the same reason `end_session_if_waiting` is: a non-atomic
/// GET-then-SET races with `/session/:code/start`. Here the single Lua
/// script reads, removes, status-checks, and writes/deletes as one operation.
/// `/start`'s own CAS (its `expected_joiner_count` guard) handles the
/// cross-operation race — if this removal lands between /start's WS-ready
/// check and its transaction, /start sees the count mismatch and retries.
pub(super) async fn remove_joiner_on_leave(
    state: &AppState,
    code: &str,
    player_id: &str,
) -> Result<(), String> {
    // lua-cjson encodes an empty Lua table as `{}` (a JSON object). If
    // removing the last joiner empties `session.joiners`, the naive re-encode
    // would write `"joiners":{}`, which then fails to deserialize into the
    // Rust `Vec<JoinerEntry>`. The `string.gsub` below forces it back to `[]`
    // in that one case.
    //
    // Safety of the literal-substring gsub: it intentionally targets the exact
    // JSON fragment lua-cjson produces for an empty `session.joiners`. No other
    // session field can contain that substring — `code` is from a brace-free
    // alphabet, `host_player_id` is digits, `gamemode`/`status` are fixed
    // lowercase words, and `player_count` is a number. Revisit this gsub if the
    // Session shape gains a free-form string field.
    const REMOVE_JOINER_ON_LEAVE_SCRIPT: &str = r#"
local raw = redis.call('GET', KEYS[1])
if not raw then
  return cjson.encode({result = 'not_found'})
end
local session = cjson.decode(raw)
local found_idx = nil
for i, j in ipairs(session.joiners) do
  if j.player_id == ARGV[1] then
    found_idx = i
    break
  end
end
if not found_idx then
  return cjson.encode({result = 'not_joiner'})
end
table.remove(session.joiners, found_idx)
if session.status ~= 'waiting' and #session.joiners == 0 then
  redis.call('DEL', KEYS[1])
  return cjson.encode({result = 'ended'})
end
local encoded = cjson.encode(session)
if #session.joiners == 0 then
  encoded = string.gsub(encoded, '"joiners":{}', '"joiners":[]')
end
redis.call('SET', KEYS[1], encoded, 'EX', tonumber(ARGV[2]))
return cjson.encode({result = 'removed'})
"#;

    #[derive(serde::Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case")]
    enum RemoveOutcome {
        Removed,
        Ended,
        NotFound,
        NotJoiner,
    }

    let mut conn = state.redis.get().await.map_err(|e| e.to_string())?;
    let script = deadpool_redis::redis::Script::new(REMOVE_JOINER_ON_LEAVE_SCRIPT);
    let raw: String = script
        .key(format!("session:{}", code))
        .arg(player_id)
        .arg(state.config.session_ttl_secs)
        .invoke_async(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    let outcome: RemoveOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("malformed lua result: {e}"))?;

    match outcome {
        RemoveOutcome::Removed => {
            tracing::info!("ws: removed joiner {} from session {}", player_id, code);
        }
        RemoveOutcome::Ended => {
            state
                .signal_hub
                .broadcast(code, ServerMsg::SessionEnded { reason: "last_player_left" }, None)
                .await;
            tracing::info!("ws: session {} ended (last peer left, host alone)", code);
        }
        RemoveOutcome::NotFound | RemoveOutcome::NotJoiner => {}
    }
    Ok(())
}
