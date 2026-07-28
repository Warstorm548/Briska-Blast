//! Chat moderation audit records.
//!
//! Four categories, each its own Redis list, matching the four tables the Chat
//! Audit Logs page renders:
//!
//! | Category | What lands here |
//! |---|---|
//! | `player` | actions on a player's chat privileges — including **automated** enforcement, which carries `group = System` |
//! | `word` | blacklist add/remove/toggle, and (later) Approve Word |
//! | `list` | moderation-list edits: un-ban, lift suspension, whitelist |
//! | `system` | automated events that are *not* enforcement — chiefly word flagging |
//!
//! # Why one record type for four tables
//!
//! The four view models in `admin::templates::chatmod` differ only in which
//! columns they surface. Storing four shapes would mean four migrations every
//! time the common spine (who / when / what / why) changes. One flat record with
//! per-category-optional fields stores once and projects at render time.
//!
//! # Snapshots are pinned, not copied
//!
//! A record does not embed the chat history. It stores the transcript instance
//! (`sid`) plus a `cut_index` — the transcript length at the moment the record
//! was written — and the `body_ids` it acted on. Rendering replays the transcript
//! up to the cut and tags those bodies. A session with 40 flagged messages
//! therefore stores the conversation once, not 40 times.
//!
//! This is only sound because transcripts are append-only. A future Delete Body
//! must write a tombstone rather than removing the entry, or every earlier
//! snapshot silently changes.
//!
//! A record with [`AuditRecord::full`] set replays the *whole* transcript instead
//! of stopping at the cut, which is how a ban shows the entire conversation
//! rather than only what preceded it. Same storage, same pinning — only the
//! range read differs, and `cut_index` becomes an action-point marker.
//!
//! # Growth
//!
//! Records have **no TTL and are never trimmed** — that is the deliberate
//! retention policy (removal will be a manual admin action, not yet built). They
//! are small, but the lists grow without bound; see the roadmap follow-up.

use deadpool_redis::redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};

use crate::admin::AdminRole;

/// Group label for automated, program-initiated records.
pub const SYSTEM_GROUP: &str = "System";

/// Display-name slot for the blacklist filter's automated entries.
pub const WORD_FILTER_SOURCE: &str = "Word Filter";

/// Which audit table a record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCategory {
    Player,
    Word,
    List,
    System,
}

impl AuditCategory {
    fn key(self) -> &'static str {
        match self {
            Self::Player => "chat:audit:player",
            Self::Word => "chat:audit:word",
            Self::List => "chat:audit:list",
            Self::System => "chat:audit:system",
        }
    }
}

/// One audit record. Fields not relevant to a category stay at their defaults and
/// are simply not projected when that category renders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditRecord {
    pub at_ms: i64,
    /// The acting moderator's display name, or the automated source (e.g.
    /// `Word Filter`) for System records.
    pub actor: String,
    /// The moderator's stable Pocket ID subject. Recorded alongside the display
    /// name because a display name can change in Pocket ID after the fact, and an
    /// audit trail that only stored the name would silently re-attribute.
    #[serde(default)]
    pub actor_sub: String,
    /// Role label (`SuperAdmin` / `Admin` / `Moderator`) or [`SYSTEM_GROUP`].
    pub group: String,
    /// What was done, e.g. `Blacklist Word`, `Flag Word`, `Ban`.
    pub action: String,
    /// Why. For System records this is the trigger that fired.
    #[serde(default)]
    pub reason: String,

    // --- subject ---
    #[serde(default)]
    pub target_username: String,
    /// The `/register`-issued counter number. `None` for records with no player
    /// subject, such as a global blacklist add.
    #[serde(default)]
    pub target_player_id: Option<u64>,
    /// Words the record concerns — one for a blacklist action, one or more for a
    /// flag.
    #[serde(default)]
    pub words: Vec<String>,
    /// Which moderation list was edited (List category only).
    #[serde(default)]
    pub list: String,

    // --- pinned evidence ---
    /// Transcript instance the snapshot replays from. Empty when there is none.
    #[serde(default)]
    pub sid: String,
    /// Transcript length when this record was written; the snapshot is the first
    /// `cut_index` lines.
    #[serde(default)]
    pub cut_index: usize,
    /// Bodies this record acted on or referenced. Always a list — any tool may
    /// cover zero, one, or several bodies.
    #[serde(default)]
    pub body_ids: Vec<String>,
    /// Render the **whole** transcript rather than truncating at `cut_index`.
    ///
    /// Set for bans only. A ban is permanent in a way no other tool is, so its
    /// record is expected to answer "what else happened here", not just "what
    /// prompted this" — `cut_index` stops being the end of the snapshot and
    /// becomes a marker showing where in the conversation the ban fell.
    ///
    /// The pinning is unchanged: still one stored transcript, still replayed
    /// rather than copied. Note this means the view keeps growing as the session
    /// continues, which is the intent.
    #[serde(default)]
    pub full: bool,

    // --- outcome ---
    /// Whether the action reached the player, for the actions that send them
    /// something. `None` where delivery is not a concept — a blacklist edit
    /// notifies nobody, so "not delivered" would be meaningless rather than bad.
    ///
    /// A warning is never queued: an offline or in-match player simply does not
    /// receive it, and this is the only place that fact survives.
    #[serde(default)]
    pub delivered: Option<bool>,
}

impl AuditRecord {
    /// A record attributed to a human moderator, stamped now.
    pub fn by_moderator(
        actor: &str,
        actor_sub: &str,
        role: AdminRole,
        action: &str,
        reason: &str,
    ) -> Self {
        Self {
            at_ms: chrono::Utc::now().timestamp_millis(),
            actor: actor.to_string(),
            actor_sub: actor_sub.to_string(),
            group: role.label().to_string(),
            action: action.to_string(),
            reason: reason.to_string(),
            ..Default::default()
        }
    }

    /// A record attributed to an automated process, stamped now. `source` fills
    /// the Display Name slot and `trigger` the Reason slot.
    pub fn by_system(source: &str, action: &str, trigger: &str) -> Self {
        Self {
            at_ms: chrono::Utc::now().timestamp_millis(),
            actor: source.to_string(),
            group: SYSTEM_GROUP.to_string(),
            action: action.to_string(),
            reason: trigger.to_string(),
            ..Default::default()
        }
    }

    /// Pin the chat as it stands: which transcript, and how much of it.
    pub fn with_snapshot(mut self, sid: &str, cut_index: usize, body_ids: Vec<String>) -> Self {
        self.sid = sid.to_string();
        self.cut_index = cut_index;
        self.body_ids = body_ids;
        self
    }

    /// Keep the entire transcript, marking the action point instead of cutting
    /// there. See [`AuditRecord::full`].
    pub fn full(mut self) -> Self {
        self.full = true;
        self
    }

    /// Attach the player this record is about.
    pub fn with_target(mut self, username: &str, player_id: Option<u64>) -> Self {
        self.target_username = username.to_string();
        self.target_player_id = player_id;
        self
    }

    pub fn with_words(mut self, words: Vec<String>) -> Self {
        self.words = words;
        self
    }

    /// Name the moderation list this record edited (List category only).
    pub fn with_list(mut self, list: &str) -> Self {
        self.list = list.to_string();
        self
    }

    /// Record whether what this action sent actually reached the player.
    pub fn with_delivery(mut self, delivered: bool) -> Self {
        self.delivered = Some(delivered);
        self
    }
}

/// Append a record. Newest first, so a plain `LRANGE 0 n` is the recent page.
///
/// Failures are logged and swallowed by callers where the moderation action has
/// already happened — losing the log line is bad, but failing the action the
/// moderator just took (and leaving them unsure whether it applied) is worse.
pub async fn write(
    conn: &mut deadpool_redis::Connection,
    category: AuditCategory,
    record: &AuditRecord,
) -> RedisResult<()> {
    let json = serde_json::to_string(record).map_err(|e| {
        deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::TypeError,
            "chat: audit serialize",
            e.to_string(),
        ))
    })?;
    conn.lpush(category.key(), json).await
}

/// Most recent `limit` records for a category, newest first.
pub async fn read(
    conn: &mut deadpool_redis::Connection,
    category: AuditCategory,
    limit: isize,
) -> RedisResult<Vec<AuditRecord>> {
    let raw: Vec<String> = conn.lrange(category.key(), 0, limit - 1).await?;
    Ok(raw
        .into_iter()
        .filter_map(|json| match serde_json::from_str::<AuditRecord>(&json) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!("chat: skipping malformed audit record: {}", e);
                None
            }
        })
        .collect())
}

/// Render a millisecond timestamp the way every audit table shows it:
/// `2026-07-24 13:58:20 UTC`.
pub fn format_timestamp(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_map_to_distinct_keys() {
        let keys = [
            AuditCategory::Player.key(),
            AuditCategory::Word.key(),
            AuditCategory::List.key(),
            AuditCategory::System.key(),
        ];
        assert_eq!(keys, [
            "chat:audit:player",
            "chat:audit:word",
            "chat:audit:list",
            "chat:audit:system",
        ]);
        // No key may shadow another's list.
        let mut sorted = keys;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
    }

    #[test]
    fn moderator_record_carries_identity_and_role() {
        let r = AuditRecord::by_moderator(
            "jeanluc",
            "pocket-id-sub-123",
            AdminRole::Moderator,
            "Blacklist Word",
            "Slur",
        );
        assert_eq!(r.actor, "jeanluc");
        assert_eq!(r.actor_sub, "pocket-id-sub-123", "sub must survive a rename");
        assert_eq!(r.group, "Moderator");
        assert_eq!(r.action, "Blacklist Word");
        assert_eq!(r.reason, "Slur");
        assert!(r.at_ms > 0);
    }

    #[test]
    fn role_label_matches_the_group_column() {
        for (role, label) in [
            (AdminRole::SuperAdmin, "SuperAdmin"),
            (AdminRole::Admin, "Admin"),
            (AdminRole::Moderator, "Moderator"),
        ] {
            let r = AuditRecord::by_moderator("x", "", role, "a", "b");
            assert_eq!(r.group, label);
        }
    }

    #[test]
    fn system_record_uses_the_system_group() {
        let r = AuditRecord::by_system(WORD_FILTER_SOURCE, "Flag Word", "Matched blacklist");
        assert_eq!(r.actor, "Word Filter");
        assert_eq!(r.group, "System");
        assert_eq!(r.reason, "Matched blacklist");
        assert!(r.actor_sub.is_empty(), "an automated record has no Pocket ID subject");
    }

    #[test]
    fn snapshot_is_pinned_not_copied() {
        let r = AuditRecord::by_system(WORD_FILTER_SOURCE, "Flag Word", "Matched blacklist")
            .with_snapshot("a00000000001", 7, vec!["a00000000005".into()]);
        assert_eq!(r.sid, "a00000000001");
        assert_eq!(r.cut_index, 7, "the snapshot is the first 7 lines");
        assert_eq!(r.body_ids, vec!["a00000000005"]);
    }

    #[test]
    fn builders_compose() {
        let r = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Admin, "Ban", "Slur spam")
            .with_target("EldenFire", Some(12))
            .with_words(vec!["frick".into()])
            .with_snapshot("a00000000001", 3, vec!["a00000000002".into()]);
        assert_eq!(r.target_username, "EldenFire");
        assert_eq!(r.target_player_id, Some(12));
        assert_eq!(r.words, vec!["frick"]);
        assert_eq!(r.cut_index, 3);
    }

    #[test]
    fn record_round_trips() {
        let r = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Admin, "Ban", "Slur spam")
            .with_target("EldenFire", Some(12))
            .with_words(vec!["frick".into()]);
        let back: AuditRecord = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.actor, r.actor);
        assert_eq!(back.target_player_id, Some(12));
        assert_eq!(back.words, vec!["frick"]);
    }

    #[test]
    fn record_deserializes_without_optional_fields() {
        let r: AuditRecord = serde_json::from_str(
            r#"{"at_ms":1753000000000,"actor":"jeanluc","group":"Admin","action":"Ban"}"#,
        )
        .unwrap();
        assert_eq!(r.actor, "jeanluc");
        assert!(r.body_ids.is_empty());
        assert!(r.words.is_empty());
        assert_eq!(r.target_player_id, None);
        assert_eq!(r.cut_index, 0);
        assert_eq!(r.delivered, None, "records predating warnings must load");
        assert!(!r.full, "records predating bans must still truncate at the cut");
    }

    /// The whole point of the flag: a ban keeps the conversation after it, every
    /// other record stops at the cut.
    #[test]
    fn only_a_full_record_asks_for_the_whole_transcript() {
        let ban = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Moderator, "Ban", "Slurs")
            .with_snapshot("a00000000001", 3, vec!["a00000000002".into()])
            .full();
        assert!(ban.full);
        assert_eq!(ban.cut_index, 3, "the cut survives as the action-point marker");

        let warn = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Moderator, "Warn", "Spam")
            .with_snapshot("a00000000001", 3, vec![]);
        assert!(!warn.full);

        let back: AuditRecord = serde_json::from_str(&serde_json::to_string(&ban).unwrap()).unwrap();
        assert!(back.full, "the flag must survive a round trip");
    }

    /// Undelivered is a distinct state from not-applicable: a warning that never
    /// landed must not read the same as a blacklist edit that notifies nobody.
    #[test]
    fn delivery_outcome_survives_a_round_trip() {
        let missed = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Moderator, "Warn", "Spam")
            .with_target("EldenFire", Some(12))
            .with_delivery(false);
        let back: AuditRecord =
            serde_json::from_str(&serde_json::to_string(&missed).unwrap()).unwrap();
        assert_eq!(back.delivered, Some(false));

        let word = AuditRecord::by_moderator(
            "jeanluc",
            "sub",
            AdminRole::Moderator,
            "Blacklist Word",
            "Slur",
        );
        assert_eq!(word.delivered, None, "a word action sends nothing");
    }

    #[test]
    fn timestamps_render_in_the_table_format() {
        assert_eq!(format_timestamp(1_784_901_500_000), "2026-07-24 13:58:20 UTC");
        // A nonsense value degrades to empty rather than panicking.
        assert_eq!(format_timestamp(i64::MAX), "");
    }
}
