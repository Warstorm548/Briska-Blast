//! Session chat transcripts — capture, retention, and teardown.
//!
//! # Why transcripts are not keyed by session code
//!
//! Session codes are recycled. `api/host.rs` only checks a candidate code
//! against *live* sessions, so `FJ5B3V` is handed out again once the session
//! holding it ends. A permanently retained transcript stored under its code would
//! therefore eventually be appended to — or shadowed by — an unrelated session
//! months later. That is the same class of bug as embedding a recyclable player
//! id in a body id (see [`super::ids`]).
//!
//! So each *instance* of a chat gets a `sid` drawn from the body-id sequence, and
//! `chat:live:{CODE}` is the short-lived pointer from the code a moderator types
//! to the instance it currently means. Because sids and body ids come from one
//! counter, a sid can never collide with another sid — or with a body id.
//!
//! # Retention
//!
//! Capture starts at a session's first message so a moderator has something live
//! to watch, and so a later trigger can retain the *whole* conversation rather
//! than only the part after it fired. What survives teardown:
//!
//! - **Nothing flagged, no moderator involvement** → deleted outright.
//! - **A blacklisted word fired** → retained, and marked flagged (the red dot).
//! - **A moderator acted, or typed a single message** → retained. "Acted" covers
//!   every non-player line: a moderator post, a warning, a ban, a deletion mark.
//!
//! Retention is one `SADD`; nothing is copied or moved. [`on_session_end`] is the
//! single decision point, called from every path that ends a session.
//!
//! # Why there is an orphan sweep
//!
//! No chat key has a TTL — `session:{CODE}` is the only key in the deployment
//! that expires. That is deliberate, but it means a session that dies by *passive*
//! TTL expiry runs no server code at all (there is no keyspace listener), and
//! would strand its transcript forever. [`sweep_orphans`] reconciles that: any
//! `chat:live:*` pointer whose session key is gone gets the normal teardown. It
//! runs on the Chat-Mod landing page, which already enumerates sessions.

use std::collections::HashMap;

use deadpool_redis::redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Code → current chat instance id. Deleted at teardown.
fn live_key(code: &str) -> String {
    format!("chat:live:{code}")
}

/// The messages themselves, oldest first.
fn transcript_key(sid: &str) -> String {
    format!("chat:transcript:{sid}")
}

/// Instance metadata: which code it was, when it ran.
fn meta_key(sid: &str) -> String {
    format!("chat:meta:{sid}")
}

/// Body id → [`DeletionMark`] for bodies withdrawn from players' view. Lives
/// beside the transcript so the transcript itself is never mutated.
fn deleted_key(sid: &str) -> String {
    format!("chat:deleted:{sid}")
}

/// Sids that survive teardown.
const RETAINED_KEY: &str = "chat:retained";

/// Sids with at least one flagged message — drives the red dot without having to
/// scan a transcript.
const FLAGGED_KEY: &str = "chat:flagged";

/// Retained sids, scored by when their session ended, for listing past sessions.
const ARCHIVES_KEY: &str = "chat:archives";

/// Who produced a line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Player,
    Moderator,
    /// A warning a moderator sent to one player. Recorded in the transcript so
    /// the intervention sits in the conversation where it happened, but **never
    /// broadcast** — only the target received it, over `ServerMsg::ChatWarning`.
    ///
    /// Field roles differ from the other kinds: `player_id`/`username` are the
    /// **target**, `text` is the reason, and `mod_user`/`mod_sub` are the acting
    /// moderator. `mod_anonymous` does not apply — a warning names no moderator
    /// on the wire, and the panel always shows the real one.
    Warning,
    /// A chat ban a moderator applied to one player. Same field roles as
    /// [`MessageKind::Warning`] — `player_id`/`username` are the target, `text`
    /// is the reason — and likewise never broadcast; only the banned player
    /// received `ServerMsg::ChatBanned`.
    ///
    /// Recorded in the transcript so the intervention sits in the conversation
    /// that prompted it, which is also what marks the point a reviewer is looking
    /// for when reading the full-transcript view of a ban record.
    Ban,
}

/// One captured chat line.
///
/// `text` is always the **original**, uncensored body. Players receive the masked
/// form (see [`super::blacklist::mask`]); the transcript keeps what was actually
/// typed so a moderator can judge it in context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub body_id: String,
    #[serde(default)]
    pub kind: MessageKind,
    /// Server-attested sender. Empty for moderator messages.
    #[serde(default)]
    pub player_id: String,
    /// Display name as broadcast — for an anonymous moderator this is `Mod`.
    #[serde(default)]
    pub username: String,
    pub text: String,
    /// Blacklisted words that fired on this line, in first-occurrence order.
    #[serde(default)]
    pub flagged_words: Vec<String>,
    /// The **real** moderator identity behind a moderator line, recorded even
    /// when the message was posted anonymously. Only the broadcast is anonymous;
    /// the record never is.
    #[serde(default)]
    pub mod_user: String,
    #[serde(default)]
    pub mod_sub: String,
    #[serde(default)]
    pub mod_anonymous: bool,
    /// Whether a [`MessageKind::Warning`] actually reached its target's client.
    /// `None` on every other kind, where delivery is not a concept.
    ///
    /// Recorded here as well as on the audit record because the two are read in
    /// different places: the panel renders the transcript echo without touching
    /// the audit log, and a moderator needs to see "this did not land" in the
    /// conversation itself, not only in a separate table.
    #[serde(default)]
    pub delivered: Option<bool>,
    pub at_ms: i64,
}

impl StoredMessage {
    pub fn is_flagged(&self) -> bool {
        !self.flagged_words.is_empty()
    }
}

/// Why a body was removed from players' view, and by whom.
///
/// Stored per body in `chat:deleted:{sid}` rather than on the [`StoredMessage`]
/// itself. The transcript list is append-only — audit records pin a cut index
/// into it, so mutating an entry in place would need an index lookup and would
/// make "append-only" a convention rather than a fact. A side key keeps the list
/// literally untouched after append.
///
/// This **is** a tombstone, the mark-don't-remove kind. Removing the entry
/// instead would shift every later index and silently corrupt every audit
/// snapshot written after it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionMark {
    /// The moderator's real display name. A deletion is never anonymous.
    pub mod_user: String,
    pub mod_sub: String,
    /// The reason given for the action that deleted it.
    pub reason: String,
    pub at_ms: i64,
}

/// Per-instance metadata, written once when the instance is minted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMeta {
    pub sid: String,
    /// The session code this instance ran under. Informational after teardown —
    /// the code may since have been reissued to a different session.
    pub code: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
}

/// Resolve the chat instance for a live session code, minting one on first use.
///
/// `SET NX` decides the winner, so two messages arriving together cannot create
/// two instances for one session. The loser's minted id is simply discarded —
/// body ids are not required to be gapless.
pub async fn sid_for(conn: &mut deadpool_redis::Connection, code: &str) -> RedisResult<String> {
    let key = live_key(code);
    if let Some(existing) = conn.get::<_, Option<String>>(&key).await? {
        return Ok(existing);
    }

    let candidate = super::ids::next_body_id(conn).await?;
    let won: bool = conn.set_nx(&key, &candidate).await?;
    if !won {
        // Someone else minted first; use theirs.
        return Ok(conn
            .get::<_, Option<String>>(&key)
            .await?
            .unwrap_or(candidate));
    }

    let meta = TranscriptMeta {
        sid: candidate.clone(),
        code: code.to_string(),
        started_at_ms: chrono::Utc::now().timestamp_millis(),
        ended_at_ms: None,
    };
    if let Ok(json) = serde_json::to_string(&meta) {
        let _: RedisResult<()> = conn.set(meta_key(&candidate), json).await;
    }
    Ok(candidate)
}

/// Append a line to a session's transcript, returning the instance id it landed
/// in. A flagged line also retains the transcript and lights the red dot.
///
/// The append and its retention markers go out as **one transaction**. Written
/// separately, a connection that died between them would leave a flagged message
/// sitting in a transcript that was never marked retained — and teardown would
/// then delete it as if nothing had happened. Losing the evidence a flag exists
/// to preserve is the one failure here that cannot be recovered from, so the two
/// writes either both land or neither does.
pub async fn append(
    conn: &mut deadpool_redis::Connection,
    code: &str,
    msg: &StoredMessage,
) -> RedisResult<String> {
    let sid = sid_for(conn, code).await?;
    let json = serde_json::to_string(msg).map_err(|e| {
        deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::TypeError,
            "chat: message serialize",
            e.to_string(),
        ))
    })?;

    let mut pipe = deadpool_redis::redis::pipe();
    pipe.atomic();
    pipe.rpush(transcript_key(&sid), json).ignore();
    if msg.is_flagged() {
        pipe.sadd(FLAGGED_KEY, &sid).ignore();
        pipe.sadd(RETAINED_KEY, &sid).ignore();
    } else if msg.kind != MessageKind::Player {
        // Any moderator involvement retains — a posted message (even an
        // anonymous one), a warning, or a ban. Testing for "not a player line"
        // rather than listing the moderator kinds is deliberate: this branch
        // previously named `Moderator` alone, so a warning in an otherwise-clean
        // session left the transcript unretained and teardown deleted the very
        // conversation the warning's audit record pinned a snapshot into.
        pipe.sadd(RETAINED_KEY, &sid).ignore();
    }
    let _: () = pipe.query_async(&mut *conn).await?;

    Ok(sid)
}

/// Mark bodies as removed from players' view, and retain the transcript.
///
/// The marks and the retention `SADD` go out as **one transaction**, for the
/// same reason [`append`] pairs its writes: a connection that died between them
/// would leave a moderated session unretained, and teardown would then delete
/// the very conversation the deletion is evidence of.
///
/// Re-marking an already-deleted body simply overwrites its mark, which keeps
/// the operation idempotent — a double-submitted form cannot corrupt anything.
pub async fn mark_deleted(
    conn: &mut deadpool_redis::Connection,
    sid: &str,
    body_ids: &[String],
    mark: &DeletionMark,
) -> RedisResult<()> {
    if body_ids.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(mark).map_err(|e| {
        deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::TypeError,
            "chat: deletion mark serialize",
            e.to_string(),
        ))
    })?;

    let mut pipe = deadpool_redis::redis::pipe();
    pipe.atomic();
    for body_id in body_ids {
        pipe.hset(deleted_key(sid), body_id, &json).ignore();
    }
    pipe.sadd(RETAINED_KEY, sid).ignore();
    pipe.query_async(&mut *conn).await
}

/// Every deletion mark on an instance, keyed by body id. Empty when none.
pub async fn deletions(
    conn: &mut deadpool_redis::Connection,
    sid: &str,
) -> RedisResult<HashMap<String, DeletionMark>> {
    let raw: HashMap<String, String> = conn.hgetall(deleted_key(sid)).await?;
    Ok(raw
        .into_iter()
        .filter_map(|(body_id, json)| match serde_json::from_str(&json) {
            Ok(mark) => Some((body_id, mark)),
            Err(e) => {
                // A corrupt mark must not resurrect the body for players — but it
                // is only a render concern here; the broadcast already happened.
                tracing::warn!("chat: skipping malformed deletion mark: {}", e);
                None
            }
        })
        .collect())
}

pub async fn is_retained(conn: &mut deadpool_redis::Connection, sid: &str) -> RedisResult<bool> {
    conn.sismember(RETAINED_KEY, sid).await
}

pub async fn is_flagged(conn: &mut deadpool_redis::Connection, sid: &str) -> RedisResult<bool> {
    conn.sismember(FLAGGED_KEY, sid).await
}

/// Decode a batch of raw entries, skipping anything corrupt rather than losing
/// the whole transcript.
fn decode(raw: Vec<String>) -> Vec<StoredMessage> {
    raw.into_iter()
        .filter_map(|json| match serde_json::from_str::<StoredMessage>(&json) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("chat: skipping malformed transcript entry: {}", e);
                None
            }
        })
        .collect()
}

/// The whole transcript, oldest first.
pub async fn all(
    conn: &mut deadpool_redis::Connection,
    sid: &str,
) -> RedisResult<Vec<StoredMessage>> {
    let raw: Vec<String> = conn.lrange(transcript_key(sid), 0, -1).await?;
    Ok(decode(raw))
}

/// The last `n` lines, oldest first — the session-card preview.
pub async fn tail(
    conn: &mut deadpool_redis::Connection,
    sid: &str,
    n: isize,
) -> RedisResult<Vec<StoredMessage>> {
    let raw: Vec<String> = conn.lrange(transcript_key(sid), -n, -1).await?;
    Ok(decode(raw))
}

/// How many lines a transcript holds. Used as the cut index when an audit record
/// pins "the chat as it stood".
pub async fn len(conn: &mut deadpool_redis::Connection, sid: &str) -> RedisResult<usize> {
    let n: isize = conn.llen(transcript_key(sid)).await?;
    Ok(n.max(0) as usize)
}

pub async fn meta(
    conn: &mut deadpool_redis::Connection,
    sid: &str,
) -> RedisResult<Option<TranscriptMeta>> {
    let raw: Option<String> = conn.get(meta_key(sid)).await?;
    Ok(raw.and_then(|json| serde_json::from_str(&json).ok()))
}

/// Resolve a live code to its instance without minting one.
pub async fn live_sid(
    conn: &mut deadpool_redis::Connection,
    code: &str,
) -> RedisResult<Option<String>> {
    conn.get(live_key(code)).await
}

/// The single teardown decision point, called from every path that ends a
/// session. Retained instances are archived; everything else is deleted outright
/// and is unrecoverable.
pub async fn on_session_end(state: &AppState, code: &str) {
    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("chat: teardown for {} skipped, no redis: {}", code, e);
            return;
        }
    };

    let sid = match live_sid(&mut conn, code).await {
        Ok(Some(sid)) => sid,
        // No chat happened in this session — nothing to clean up.
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("chat: teardown for {} could not resolve sid: {}", code, e);
            return;
        }
    };

    let retained = is_retained(&mut conn, &sid).await.unwrap_or_else(|e| {
        // Fail safe: if we cannot tell, keep the data. Deleting evidence on a
        // transient Redis error is the far worse outcome.
        tracing::warn!("chat: retention check failed for {}, keeping: {}", sid, e);
        true
    });

    if retained {
        let now = chrono::Utc::now().timestamp_millis();
        if let Ok(Some(mut m)) = meta(&mut conn, &sid).await {
            m.ended_at_ms = Some(now);
            if let Ok(json) = serde_json::to_string(&m) {
                let _: RedisResult<()> = conn.set(meta_key(&sid), json).await;
            }
        }
        let _: RedisResult<()> = conn.zadd(ARCHIVES_KEY, &sid, now).await;
        tracing::info!(session = %code, sid = %sid, "chat: transcript retained");
    } else {
        // Every key this instance owns goes here. Nothing in this module has a
        // TTL, so a key missed on this branch leaks for the life of the
        // deployment. A discarded instance cannot have deletion marks — marking
        // retains — but delete the key anyway rather than rely on that.
        let _: RedisResult<()> = conn.del(transcript_key(&sid)).await;
        let _: RedisResult<()> = conn.del(meta_key(&sid)).await;
        let _: RedisResult<()> = conn.del(deleted_key(&sid)).await;
        let _: RedisResult<()> = conn.srem(FLAGGED_KEY, &sid).await;
        tracing::debug!(session = %code, sid = %sid, "chat: transcript discarded");
    }

    let _: RedisResult<()> = conn.del(live_key(code)).await;
}

/// Reconcile transcripts whose session died without running any teardown code —
/// principally passive TTL expiry of `session:{CODE}`.
///
/// Returns how many instances it cleaned up. Uses `KEYS`, matching the existing
/// admin enumeration in `admin/dashboard.rs`; both should move to `SCAN` together
/// once the key space justifies it.
pub async fn sweep_orphans(state: &AppState) -> usize {
    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let pointers: Vec<String> = match conn.keys("chat:live:*").await {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("chat: orphan sweep failed to enumerate: {}", e);
            return 0;
        }
    };

    let mut orphans = Vec::new();
    for key in pointers {
        let Some(code) = key.strip_prefix("chat:live:") else {
            continue;
        };
        let alive: bool = conn
            .exists(format!("session:{code}"))
            .await
            .unwrap_or(true); // on error assume alive — never delete on a guess
        if !alive {
            orphans.push(code.to_string());
        }
    }

    // Drop the connection before re-entering on_session_end, which takes its own.
    drop(conn);
    for code in &orphans {
        tracing::info!(session = %code, "chat: sweeping orphaned transcript");
        on_session_end(state, code).await;
    }
    orphans.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_line(text: &str, flagged: &[&str]) -> StoredMessage {
        StoredMessage {
            body_id: "a00000000001".into(),
            kind: MessageKind::Player,
            player_id: "000000007".into(),
            username: "Warstorm".into(),
            text: text.into(),
            flagged_words: flagged.iter().map(|w| w.to_string()).collect(),
            mod_user: String::new(),
            mod_sub: String::new(),
            mod_anonymous: false,
            delivered: None,
            at_ms: 1_753_000_000_000,
        }
    }

    #[test]
    fn stored_message_round_trips() {
        let msg = player_line("frick you all", &["frick"]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: StoredMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.body_id, msg.body_id);
        assert_eq!(back.text, "frick you all");
        assert_eq!(back.flagged_words, vec!["frick"]);
        assert_eq!(back.kind, MessageKind::Player);
        assert!(back.is_flagged());
    }

    #[test]
    fn kind_serializes_snake_case_on_the_wire() {
        let json = serde_json::to_string(&MessageKind::Moderator).unwrap();
        assert_eq!(json, r#""moderator""#);
        assert_eq!(serde_json::to_string(&MessageKind::Player).unwrap(), r#""player""#);
        assert_eq!(serde_json::to_string(&MessageKind::Warning).unwrap(), r#""warning""#);
    }

    /// A warning inverts the usual field roles: the player slots name the
    /// *target*, and the moderator slots the actor. Getting this backwards would
    /// attribute the warning to the person who received it.
    #[test]
    fn warning_line_names_the_target_and_the_acting_moderator() {
        let msg = StoredMessage {
            body_id: "a00000000003".into(),
            kind: MessageKind::Warning,
            player_id: "000000007".into(),
            username: "Warstorm".into(),
            text: "Offensive language".into(),
            flagged_words: vec![],
            mod_user: "jeanluc".into(),
            mod_sub: "pocket-id-sub-123".into(),
            mod_anonymous: false,
            delivered: Some(false),
            at_ms: 1_753_000_000_000,
        };
        let back: StoredMessage =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(back.username, "Warstorm", "the player slot is the target");
        assert_eq!(back.mod_user, "jeanluc", "the mod slot is the actor");
        assert_eq!(back.text, "Offensive language", "the body is the reason");
        assert_eq!(back.delivered, Some(false));
    }

    /// Delivery is only a concept for warnings; every other kind leaves it unset
    /// so the panel can tell "did not land" apart from "not applicable".
    #[test]
    fn delivery_is_unset_on_ordinary_lines() {
        assert_eq!(player_line("nice shot", &[]).delivered, None);
        let old: StoredMessage = serde_json::from_str(
            r#"{"body_id":"a00000000001","text":"hi","at_ms":1753000000000}"#,
        )
        .unwrap();
        assert_eq!(old.delivered, None, "records predating warnings must load");
    }

    #[test]
    fn deletion_mark_round_trips() {
        let mark = DeletionMark {
            mod_user: "jeanluc".into(),
            mod_sub: "pocket-id-sub-123".into(),
            reason: "Offensive language".into(),
            at_ms: 1_753_000_000_000,
        };
        let back: DeletionMark =
            serde_json::from_str(&serde_json::to_string(&mark).unwrap()).unwrap();
        assert_eq!(back.mod_user, "jeanluc");
        assert_eq!(back.reason, "Offensive language");
    }

    /// The deletion marks live under their own key, separate from the transcript
    /// list and from every other per-instance key. A collision would either
    /// clobber the conversation or resurrect deleted bodies.
    #[test]
    fn deleted_key_is_distinct_from_the_other_instance_keys() {
        let sid = "a00000000001";
        let keys = [
            transcript_key(sid),
            meta_key(sid),
            deleted_key(sid),
            live_key("FJ5B3V"),
        ];
        assert_eq!(deleted_key(sid), "chat:deleted:a00000000001");
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
    }

    #[test]
    fn message_deserializes_without_optional_fields() {
        // A minimal record must still load — the optional fields all carry
        // defaults so an older entry never blanks a transcript.
        let msg: StoredMessage = serde_json::from_str(
            r#"{"body_id":"a00000000001","text":"hi","at_ms":1753000000000}"#,
        )
        .unwrap();
        assert_eq!(msg.kind, MessageKind::Player, "kind must default to player");
        assert!(msg.flagged_words.is_empty());
        assert!(!msg.is_flagged());
        assert!(!msg.mod_anonymous);
    }

    #[test]
    fn retention_triggers_are_exactly_flagged_or_moderator() {
        // These two predicates are what `append`'s transaction branches on, so
        // the atomic write covers precisely the lines that must not survive
        // without their retention marker.
        let flagged = player_line("frick you all", &["frick"]);
        assert!(flagged.is_flagged());
        assert_eq!(flagged.kind, MessageKind::Player);

        let clean = player_line("nice shot", &[]);
        assert!(!clean.is_flagged());
        assert_eq!(clean.kind, MessageKind::Player, "retains nothing");

        let mut moderator = player_line("keep it civil", &[]);
        moderator.kind = MessageKind::Moderator;
        assert!(!moderator.is_flagged());
        assert_eq!(moderator.kind, MessageKind::Moderator, "retains on kind alone");
    }

    #[test]
    fn unflagged_message_is_not_flagged() {
        assert!(!player_line("nice shot", &[]).is_flagged());
    }

    #[test]
    fn anonymous_moderator_line_still_records_the_real_identity() {
        // The load-bearing accountability property: only the broadcast is
        // anonymous, never the record.
        let msg = StoredMessage {
            body_id: "a00000000002".into(),
            kind: MessageKind::Moderator,
            player_id: String::new(),
            username: "Mod".into(),
            text: "keep it civil".into(),
            flagged_words: vec![],
            mod_user: "jeanluc".into(),
            mod_sub: "pocket-id-sub-123".into(),
            mod_anonymous: true,
            delivered: None,
            at_ms: 1_753_000_000_000,
        };
        let back: StoredMessage =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(back.username, "Mod", "players saw the generic name");
        assert_eq!(back.mod_user, "jeanluc", "the record kept the real one");
        assert_eq!(back.mod_sub, "pocket-id-sub-123");
        assert!(back.mod_anonymous);
    }

    #[test]
    fn meta_round_trips_with_and_without_an_end_time() {
        let live = TranscriptMeta {
            sid: "a00000000001".into(),
            code: "FJ5B3V".into(),
            started_at_ms: 1_753_000_000_000,
            ended_at_ms: None,
        };
        let back: TranscriptMeta =
            serde_json::from_str(&serde_json::to_string(&live).unwrap()).unwrap();
        assert_eq!(back.code, "FJ5B3V");
        assert!(back.ended_at_ms.is_none());

        let ended = TranscriptMeta { ended_at_ms: Some(1_753_000_100_000), ..live };
        let back: TranscriptMeta =
            serde_json::from_str(&serde_json::to_string(&ended).unwrap()).unwrap();
        assert_eq!(back.ended_at_ms, Some(1_753_000_100_000));
    }

    #[test]
    fn decode_skips_corrupt_entries_without_losing_the_rest() {
        let good = serde_json::to_string(&player_line("one", &[])).unwrap();
        let also_good = serde_json::to_string(&player_line("two", &[])).unwrap();
        let decoded = decode(vec![good, "{not json".into(), also_good]);
        assert_eq!(decoded.len(), 2, "one bad entry must not lose the transcript");
        assert_eq!(decoded[0].text, "one");
        assert_eq!(decoded[1].text, "two");
    }

    #[test]
    fn keys_are_namespaced_per_instance() {
        assert_eq!(live_key("FJ5B3V"), "chat:live:FJ5B3V");
        assert_eq!(transcript_key("a00000000001"), "chat:transcript:a00000000001");
        assert_eq!(meta_key("a00000000001"), "chat:meta:a00000000001");
    }
}
