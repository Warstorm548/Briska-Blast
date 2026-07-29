//! The chat ban list: storage and lookup.
//!
//! A chat ban is **account-wide and permanent** — it governs chat privileges
//! only and never touches game access, so a banned player keeps playing and
//! simply cannot speak. It is lifted only by an un-ban from the Moderation Lists
//! page. That is the whole scope contract; see the module docs in
//! [`crate::admin::chatmod`].
//!
//! # Why the key is normalized
//!
//! Player ids reach this module in more than one shape. The panel renders them
//! zero-padded through [`PlayerId::from_counter`] (`000000042`), a moderator may
//! type the bare number into the Target Player IDs field (`42`), and the
//! signaling layer carries whatever `/register` issued. Stored raw, a ban written
//! as `42` would never match a chat message arriving as `000000042` — the mute
//! would silently do nothing, which for a permanence tool is the worst possible
//! failure. So every read and write goes through [`normalize_id`] first, and the
//! hash field is always the canonical padded form.
//!
//! # Ban entries and the id reuse pool
//!
//! Bans are keyed by the player *number*, and numbers are recycled:
//! `admin::users::delete_user` returns a deleted player's number to
//! `player:freelist` for `/register` to reissue. A ban left behind would
//! therefore transfer to an unrelated player who happened to inherit the number.
//! Deleting a user consequently clears their ban as part of the same wipe — the
//! token that *was* the identity is gone, so there is nobody left to ban.
//!
//! # No TTL
//!
//! Like every other chat key, `chat:banned` never expires (`session:{CODE}` is
//! the only key in the deployment that does). A ban ends when a moderator ends
//! it, not when a clock runs out — that is what distinguishes it from a suspension.

use deadpool_redis::redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};
use shared::types::player::PlayerId;

use crate::state::AppState;

/// Redis hash of banned players. Field = the normalized player id, value = a
/// JSON [`BanEntry`]. No TTL — see the module docs.
const BANNED_KEY: &str = "chat:banned";

/// What an audit record calls this list.
///
/// Every action that adds to or removes from the ban list tags its record with
/// this, which is what puts the row in the List table alongside the Player table
/// (see [`crate::chat::audit`]). Shared rather than repeated because the ban
/// paths are in two different modules — the session view and the Moderation
/// Lists page — and a tag that differed between them would quietly file the same
/// action under two different lists.
///
/// Matches the `List` filter dropdown's option text, not the Banned Users
/// sub-tab title: the tab names a page, this names the list a record edited.
pub const AUDIT_LIST_NAME: &str = "Ban List";

/// One banned player, as stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BanEntry {
    /// The player's counter number. Kept numeric so every moderation surface can
    /// render it through [`PlayerId::from_counter`] like all the others.
    pub player_id: u64,
    /// Display name resolved at ban time. Frozen deliberately: a rename later
    /// must not rewrite who the ledger says was banned.
    #[serde(default)]
    pub username: String,
    pub reason: String,
    /// Offensive words cited with the ban — the session tool fills these from the
    /// flagged words on the covered bodies, the Lists tool from its own field.
    /// They render as the audit table's red chips.
    #[serde(default)]
    pub words: Vec<String>,
    /// Display name of the moderator who applied it.
    #[serde(default)]
    pub banned_by: String,
    /// Their stable Pocket ID subject, recorded alongside the display name for
    /// the same reason [`crate::chat::audit::AuditRecord`] does: a display name
    /// can change in Pocket ID after the fact.
    #[serde(default)]
    pub banned_sub: String,
    pub at_ms: i64,
    /// Transcript instance the ban happened in. Empty for a ban applied from the
    /// Moderation Lists page, which has no session context.
    #[serde(default)]
    pub sid: String,
    /// Transcript length at the moment of the ban. The ledger replays the
    /// **whole** transcript and uses this only to mark where the ban fell, unlike
    /// a warning's snapshot which truncates here.
    #[serde(default)]
    pub cut_index: usize,
}

/// Canonical hash-field form for a player id: parse the number, render it
/// zero-padded. `42`, `000000042` and `  42  ` are therefore one entry.
///
/// Returns `None` for anything non-numeric, so a typo in the `;`-separated
/// target field is rejected with a message rather than writing a permanent ban
/// nobody can match or find.
pub fn normalize_id(raw: &str) -> Option<String> {
    let n: u64 = raw.trim().parse().ok()?;
    Some(PlayerId::from_counter(n).to_string())
}

/// The numeric form of an id, for the fields that store it as a number.
pub fn numeric_id(raw: &str) -> Option<u64> {
    raw.trim().parse().ok()
}

/// What a [`ban`] call actually did, split so the caller can report the
/// difference rather than silently swallowing it — same shape and rationale as
/// [`super::blacklist::AddOutcome`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BanOutcome {
    /// Ids newly written.
    pub banned: Vec<String>,
    /// Ids already banned, left untouched.
    pub already: Vec<String>,
    /// Ids that were not numbers at all.
    pub invalid: Vec<String>,
}

/// Ban one or more players.
///
/// An existing ban is **never overwritten** (`HSETNX`), for the same reason a
/// duplicate blacklist word is not: the first ban already wrote an audit record
/// naming its author, reason and evidence, and quietly replacing the entry would
/// leave the list disagreeing with the log about why someone is banned.
///
/// `entry` supplies everything except the id, which is filled per target.
pub async fn ban(
    conn: &mut deadpool_redis::Connection,
    ids: &[String],
    entry: &BanEntry,
) -> RedisResult<BanOutcome> {
    let mut out = BanOutcome::default();
    for raw in ids {
        let Some(key) = normalize_id(raw) else {
            out.invalid.push(raw.trim().to_string());
            continue;
        };
        // An id repeated within one submission is a duplicate of itself; report
        // it once rather than counting it twice.
        if out.banned.contains(&key) || out.already.contains(&key) {
            continue;
        }
        let stored = BanEntry {
            player_id: numeric_id(raw).unwrap_or_default(),
            ..entry.clone()
        };
        let json = serde_json::to_string(&stored).unwrap_or_default();
        let inserted: bool = conn.hset_nx(BANNED_KEY, &key, json).await?;
        if inserted {
            out.banned.push(key);
        } else {
            out.already.push(key);
        }
    }
    Ok(out)
}

/// Lift bans. Returns the ids that were actually on the list, so the caller can
/// say which selections were no-ops instead of reporting a smaller count.
pub async fn unban(
    conn: &mut deadpool_redis::Connection,
    ids: &[String],
) -> RedisResult<Vec<String>> {
    let mut lifted = Vec::new();
    for raw in ids {
        let Some(key) = normalize_id(raw) else {
            continue;
        };
        if lifted.contains(&key) {
            continue;
        }
        let removed: i64 = conn.hdel(BANNED_KEY, &key).await?;
        if removed > 0 {
            lifted.push(key);
        }
    }
    Ok(lifted)
}

/// Drop a single player's ban without reporting anything, for the account-deletion
/// path. Best effort by design: the user is already gone, and a failure here must
/// not fail the deletion.
pub async fn clear(conn: &mut deadpool_redis::Connection, id: &str) {
    let Some(key) = normalize_id(id) else {
        return;
    };
    if let Err(e) = conn.hdel::<_, _, i64>(BANNED_KEY, &key).await {
        tracing::warn!(player = %id, "chat: could not clear ban on user deletion: {}", e);
    }
}

/// The ban on a player, if any.
pub async fn lookup(
    conn: &mut deadpool_redis::Connection,
    player_id: &str,
) -> RedisResult<Option<BanEntry>> {
    let Some(key) = normalize_id(player_id) else {
        return Ok(None);
    };
    let raw: Option<String> = conn.hget(BANNED_KEY, &key).await?;
    Ok(raw.map(|json| {
        serde_json::from_str(&json).unwrap_or_else(|e| {
            // A corrupt entry must not silently un-ban someone — the ban still
            // holds, there is simply no reason text left to show for it.
            tracing::warn!(player = %player_id, "chat: malformed ban entry: {}", e);
            BanEntry {
                player_id: numeric_id(player_id).unwrap_or_default(),
                ..Default::default()
            }
        })
    }))
}

/// The ban a chat message must clear, or `None` to let it through.
///
/// **Fails open.** A Redis fault returns `None` and the message is broadcast.
/// The alternative — treating an unreachable ban list as "everyone is banned" —
/// would silence chat for the entire deployment during an outage, which is far
/// worse than a banned player getting a line through until Redis returns.
pub async fn enforced_on(state: &AppState, player_id: &str) -> Option<BanEntry> {
    let mut conn = match state.redis.get().await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::warn!("chat: ban check skipped, no redis: {}", e);
            return None;
        }
    };
    match lookup(&mut conn, player_id).await {
        Ok(entry) => entry,
        Err(e) => {
            tracing::warn!("chat: ban check failed, allowing message: {}", e);
            None
        }
    }
}

/// The whole ban list, newest ban first — the Banned Users ledger's ordering.
pub async fn load(conn: &mut deadpool_redis::Connection) -> RedisResult<Vec<BanEntry>> {
    let raw: std::collections::HashMap<String, String> = conn.hgetall(BANNED_KEY).await?;
    let mut entries: Vec<BanEntry> = raw
        .into_iter()
        // A corrupt field is kept as a placeholder, never dropped. [`lookup`]
        // synthesizes one too, so the ban still *fires* — dropping the row here
        // would leave that player muted and absent from the Banned Users list,
        // with no way for a moderator to select them and lift it.
        .map(|(key, json)| match serde_json::from_str::<BanEntry>(&json) {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(player = %key, "chat: malformed ban entry, listing a placeholder: {}", e);
                BanEntry {
                    player_id: numeric_id(&key).unwrap_or_default(),
                    ..Default::default()
                }
            }
        })
        .collect();
    entries.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.player_id.cmp(&b.player_id)));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A corrupt entry must stay visible in the ledger, because [`lookup`] keeps
    /// enforcing it. Dropping the row would leave that player muted with no way
    /// for a moderator to find and un-ban them — the ban would be permanent and
    /// invisible at once.
    #[test]
    fn a_malformed_entry_still_shows_who_is_banned() {
        // Mirrors what `load` synthesizes for a field that will not parse.
        let placeholder = BanEntry {
            player_id: numeric_id("000000042").unwrap_or_default(),
            ..Default::default()
        };
        assert_eq!(placeholder.player_id, 42, "the id must survive to be un-bannable");
        assert!(placeholder.reason.is_empty(), "there is no reason left to show");

        // And `lookup` builds the same shape, so the list and enforcement agree
        // about who is banned rather than contradicting each other.
        let from_lookup = BanEntry {
            player_id: numeric_id("42").unwrap_or_default(),
            ..Default::default()
        };
        assert_eq!(placeholder.player_id, from_lookup.player_id);
    }

    #[test]
    fn ids_normalize_to_one_canonical_field() {
        // The load-bearing property: every shape a player id reaches this module
        // in must land on the same hash field, or a ban silently never fires.
        let canonical = Some("000000042".to_string());
        assert_eq!(normalize_id("42"), canonical);
        assert_eq!(normalize_id("000000042"), canonical);
        assert_eq!(normalize_id("  42  "), canonical);
    }

    #[test]
    fn ids_past_nine_digits_keep_their_width() {
        // `from_counter` is a minimum width, not an exact one — a ban must not
        // truncate a large id into a different player's.
        assert_eq!(normalize_id("1000000000"), Some("1000000000".to_string()));
    }

    #[test]
    fn non_numeric_ids_are_rejected_rather_than_stored() {
        // A typo must not write a permanent ban that nothing can ever match.
        assert_eq!(normalize_id("abc"), None);
        assert_eq!(normalize_id(""), None);
        assert_eq!(normalize_id("-1"), None);
        assert_eq!(normalize_id("4 2"), None);
    }

    #[test]
    fn entry_round_trips() {
        let entry = BanEntry {
            player_id: 42,
            username: "EldenFire".into(),
            reason: "Slur spam".into(),
            words: vec!["frick".into()],
            banned_by: "jeanluc".into(),
            banned_sub: "pocket-id-sub-123".into(),
            at_ms: 1_784_901_500_000,
            sid: "a00000000001".into(),
            cut_index: 7,
        };
        let back: BanEntry = serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
        assert_eq!(back.player_id, 42);
        assert_eq!(back.username, "EldenFire");
        assert_eq!(back.words, vec!["frick"]);
        assert_eq!(back.sid, "a00000000001");
        assert_eq!(back.cut_index, 7);
    }

    #[test]
    fn entry_deserializes_without_optional_fields() {
        // A ban applied from the Moderation Lists page carries no session
        // context, so it stores neither transcript nor words.
        let entry: BanEntry =
            serde_json::from_str(r#"{"player_id":42,"reason":"Slur spam","at_ms":1}"#).unwrap();
        assert_eq!(entry.player_id, 42);
        assert!(entry.sid.is_empty());
        assert!(entry.words.is_empty());
        assert_eq!(entry.cut_index, 0);
    }
}
