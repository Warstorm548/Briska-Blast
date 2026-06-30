pub mod host;
pub mod join;
pub mod me;
pub mod register;
pub mod session;
pub mod start;

use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::types::gamemode::GameMode;
use shared::types::session::SessionStatus;
use shared::types::spawn_settings::SpawnSettings;
use shared::types::win_condition::WinCondition;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinerEntry {
    pub player_id: String,
    pub joined_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub code: String,
    pub host_player_id: String,
    pub gamemode: GameMode,
    /// Match-end rule chosen by the host (set after host setup, like `gamemode`),
    /// broadcast to joiners and enforced server-side. `#[serde(default)]` keeps a
    /// session written before this field existed readable across a deploy (the
    /// object round-trips cleanly through the lua `cjson` scripts — only empty
    /// arrays hit the `seat_order`/`joiners` `{}` re-encode quirk, not objects).
    #[serde(default)]
    pub win_condition: WinCondition,
    /// Random-spawn rules chosen by the host (BallSpliter cadence + chain-split).
    /// `#[serde(default)]` keeps a session written before this field existed
    /// readable across a deploy.
    #[serde(default)]
    pub spawn_settings: SpawnSettings,
    pub player_count: u8,
    pub joiners: Vec<JoinerEntry>,
    pub status: SessionStatus,
    /// Frozen seating roster (`[host, ...joiners]` in join order) snapshotted
    /// once when the host calls `/start`. Empty while Waiting. Never mutated
    /// afterwards — a host promotion reorders `joiners`/`host_player_id`, but
    /// this snapshot is left intact so every client (including a process-death
    /// rejoiner, whose live session state may post-date a promotion) derives the
    /// identical Extended-mode portal layout. Consumed via the `Identified`
    /// frame's `seat_order`; see `start.rs` and `GameScene.BuildEdges`.
    #[serde(default, deserialize_with = "deserialize_seat_order")]
    pub seat_order: Vec<String>,
}

/// Deserialize `seat_order` tolerantly. Redis's lua-cjson can't tell an empty
/// array from an empty object, so any Lua script that round-trips a session while
/// `seat_order` is empty re-encodes `[]` as `{}` — most importantly the join
/// script, which runs in the Waiting phase when `seat_order` is *always* empty
/// (it's only frozen at `/start`). A plain `Vec<String>` deserialize chokes on
/// that `{}` and turns the join into a 500. Accept a real array normally and
/// treat anything else (the cjson empty-object, or null) as an empty roster.
/// This covers every path that reads a `Session` at once and self-heals a session
/// already stored with `"seat_order":{}`, rather than relying on a per-script
/// `string.gsub` like `remove_joiner_on_leave` uses for `joiners` (the omission
/// of which for this newer field is exactly what caused the 500).
fn deserialize_seat_order<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SeatOrder {
        List(Vec<String>),
        // lua-cjson's empty-table-as-object (`{}`), or an explicit null.
        Empty(serde::de::IgnoredAny),
    }
    Ok(match SeatOrder::deserialize(deserializer)? {
        SeatOrder::List(v) => v,
        SeatOrder::Empty(_) => Vec::new(),
    })
}

impl Session {
    pub fn current_player_count(&self) -> u8 {
        // Host plus joiners. Joiner count is capped at u8::MAX in practice
        // because player_count is u8 and is_full() enforces the upper bound
        // before any append.
        1u8.saturating_add(self.joiners.len() as u8)
    }

    pub fn is_full(&self) -> bool {
        self.current_player_count() >= self.player_count
    }

    pub fn contains_player(&self, player_id: &str) -> bool {
        self.host_player_id == player_id
            || self.joiners.iter().any(|j| j.player_id == player_id)
    }
}

/// Monotonic high-water counter for issued player ids. Only ever `INCR`'d —
/// never decremented — so the dashboard's "players registered" figure keeps
/// climbing even as ids are deleted and recycled.
pub(crate) const PLAYER_COUNTER_KEY: &str = "player:counter";

/// Sorted-set pool of freed player-id numbers (member == score == the numeric
/// counter value). Admin user-deletion pushes the freed number here; `/register`
/// pops the **lowest** one (`ZPOPMIN`) before falling back to the counter, so
/// deleted id numbers are reissued lowest-first to the next new player.
pub(crate) const FREELIST_KEY: &str = "player:freelist";

pub(crate) fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) async fn validate_player(
    conn: &mut deadpool_redis::Connection,
    player_id: &str,
    secret_token: &str,
) -> Result<()> {
    let stored: Option<String> = conn
        .get(format!("player:{}:token_hash", player_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let stored = stored.ok_or(AppError::Unauthorized)?;

    if hash_token(secret_token) != stored {
        return Err(AppError::Unauthorized);
    }

    Ok(())
}

/// Resolve the server-stored display names for `ids` in one round-trip,
/// returning a `player_id -> username` map. Ids with no username on file are
/// **omitted** (so the client falls back to `Player <id>`), and a Redis error
/// degrades to an empty map rather than failing — a missing display name must
/// never break a signaling connection. Used to label the lobby roster and
/// in-game scoreboard by username instead of the internal numeric id.
pub(crate) async fn fetch_usernames(
    conn: &mut deadpool_redis::Connection,
    ids: &[String],
) -> HashMap<String, String> {
    if ids.is_empty() {
        return HashMap::new();
    }

    let keys: Vec<String> = ids
        .iter()
        .map(|id| format!("player:{}:username", id))
        .collect();

    let values: Vec<Option<String>> = match conn.mget(&keys).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("fetch_usernames: mget failed, falling back to ids: {}", e);
            return HashMap::new();
        }
    };

    // Zip ids back with their values; keep only ids that had a stored name.
    // Filtering nils here means the wire map never carries an empty string,
    // which the client would otherwise render as a blank name.
    ids.iter()
        .zip(values)
        .filter_map(|(id, name)| name.map(|n| (id.clone(), n)))
        .collect()
}

pub(crate) fn client_ip(
    connect_info: &ConnectInfo<SocketAddr>,
    headers: &axum::http::HeaderMap,
) -> IpAddr {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| connect_info.0.ip())
}

#[cfg(test)]
mod tests {
    use super::Session;

    fn session_json(seat_order: &str) -> String {
        format!(
            r#"{{"code":"ABC123","host_player_id":"1","gamemode":"extended",
               "player_count":4,"joiners":[],"status":"waiting","seat_order":{seat_order}}}"#
        )
    }

    // Regression: Redis lua-cjson re-encodes an empty `seat_order` (`[]`) as the
    // JSON object `{}` whenever the join script round-trips a Waiting session.
    // A naive `Vec<String>` deserialize 500'd the join on that `{}`. We must
    // accept it as an empty roster.
    #[test]
    fn seat_order_tolerates_cjson_empty_object() {
        let session: Session = serde_json::from_str(&session_json("{}")).unwrap();
        assert!(session.seat_order.is_empty());
    }

    #[test]
    fn seat_order_reads_a_real_array() {
        let session: Session =
            serde_json::from_str(&session_json(r#"["1","2","3"]"#)).unwrap();
        assert_eq!(session.seat_order, vec!["1", "2", "3"]);
    }

    #[test]
    fn seat_order_empty_array_and_missing_both_yield_empty() {
        let from_array: Session = serde_json::from_str(&session_json("[]")).unwrap();
        assert!(from_array.seat_order.is_empty());

        // Field absent entirely → `#[serde(default)]`.
        let no_field = r#"{"code":"ABC123","host_player_id":"1","gamemode":"extended",
            "player_count":4,"joiners":[],"status":"waiting"}"#;
        let from_missing: Session = serde_json::from_str(no_field).unwrap();
        assert!(from_missing.seat_order.is_empty());
    }

    // `win_condition` is an object (`{"kind":"set_score","target":N}`), never an
    // empty array, so it can't hit the lua-cjson empty-table-as-`{}` pitfall that
    // bit `joiners`/`seat_order`. These pin that down at the Session boundary.
    #[test]
    fn win_condition_present_round_trips() {
        let json = r#"{"code":"ABC123","host_player_id":"1","gamemode":"extended",
            "win_condition":{"kind":"set_score","target":25},"player_count":4,
            "joiners":[],"status":"waiting","seat_order":[]}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.win_condition.target(), 25);
    }

    #[test]
    fn win_condition_missing_defaults_to_set_score_11() {
        // A session written before the field existed (mid-deploy) must still read.
        let no_field = r#"{"code":"ABC123","host_player_id":"1","gamemode":"extended",
            "player_count":4,"joiners":[],"status":"waiting","seat_order":[]}"#;
        let session: Session = serde_json::from_str(no_field).unwrap();
        assert_eq!(session.win_condition.target(), 11);
    }

    // Regression for the class of bug that 500'd joins twice: reproduce the EXACT
    // shape the JOIN lua script returns for a freshly-joined Waiting session — a
    // populated `joiners`, an empty `seat_order` re-encoded by lua-cjson as the
    // object `{}`, AND the new `win_condition` object all at once. If this
    // deserializes, /join won't 500 on the win_condition addition.
    #[test]
    fn join_script_output_shape_with_win_condition_deserializes() {
        let join_output = r#"{"code":"ABC123","host_player_id":"1","gamemode":"extended",
            "win_condition":{"kind":"set_score","target":11},"player_count":4,
            "joiners":[{"player_id":"2","joined_at_ms":1700000000000}],
            "status":"waiting","seat_order":{}}"#;
        let session: Session = serde_json::from_str(join_output).unwrap();
        assert_eq!(session.joiners.len(), 1);
        assert!(session.seat_order.is_empty());
        assert_eq!(session.win_condition.target(), 11);
    }
}
