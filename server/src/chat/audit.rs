//! Chat moderation audit records.
//!
//! # One record, many views
//!
//! A record is stored **once**, under `chat:audit:rec:{event_id}`. The four
//! category keys are *indexes* — lists of event ids, newest first — and the four
//! tables on the Chat Audit Logs page each render one of them.
//!
//! | Index | What it points at |
//! |---|---|
//! | `player` | actions on a player's chat privileges — including **automated** enforcement, which carries `group = System` |
//! | `word` | blacklist add/remove/toggle, and (later) Approve Word |
//! | `list` | **derived** — every record carrying a [`AuditRecord::list`] tag, whatever its home index |
//! | `system` | automated events that are *not* enforcement — chiefly word flagging |
//!
//! The list index is what makes this worth doing. A ban is both an action on a
//! player and an edit to the Ban List, and it belongs in both tables — but
//! storing it twice would mean two rows that can disagree, and counting it twice
//! in any per-player total. So it is stored once in the player index and *also
//! pointed at* from the list index. Two views, one truth.
//!
//! This generalizes because **every list the List table covers is a list of
//! players** — Ban List, Suspensions, Whitelisted Users. Each of their edits is
//! a player action that happens to change a list, so [`write`] needs no
//! per-action special cases: set [`AuditRecord::list`] and the row appears in
//! both places.
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
//! are small, but the record store and the indexes both grow without bound; see
//! the roadmap follow-up. Deletion, when it comes, must remove the record *and*
//! its id from every index, or the leftover pointer renders as a missing row.
//!
//! Reads are windowed rather than capped: [`read`] takes index positions, so the
//! cost of a page is the size of the window and not how far back it sits.

use deadpool_redis::redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};

use super::ids;
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
    /// The index list for this category — event ids, newest first. Holds
    /// pointers, never records.
    fn key(self) -> &'static str {
        match self {
            Self::Player => "chat:audit:idx:player",
            Self::Word => "chat:audit:idx:word",
            Self::List => "chat:audit:idx:list",
            Self::System => "chat:audit:idx:system",
        }
    }

    /// The pre-0.34.0 key, where whole records were stored inline.
    ///
    /// Read only by [`migrate`], and never written to again — leaving these
    /// lists untouched is what keeps a rollback to an older server working
    /// against a migrated Redis.
    fn legacy_key(self) -> &'static str {
        match self {
            Self::Player => "chat:audit:player",
            Self::Word => "chat:audit:word",
            Self::List => "chat:audit:list",
            Self::System => "chat:audit:system",
        }
    }

    /// Every category, for the migration and for tests that must cover all of
    /// them rather than the ones someone remembered.
    const ALL: [Self; 4] = [Self::Player, Self::Word, Self::List, Self::System];
}

/// Key prefix for the stored records themselves.
const RECORD_PREFIX: &str = "chat:audit:rec:";

/// Marker set once [`migrate`] has completed a full pass.
const MIGRATION_MARKER: &str = "chat:audit:migrated";

fn record_key(event_id: &str) -> String {
    format!("{RECORD_PREFIX}{event_id}")
}

/// One audit record. Fields not relevant to a category stay at their defaults and
/// are simply not projected when that category renders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditRecord {
    /// This record's own id, from the audit sequence in [`crate::chat::ids`].
    ///
    /// Assigned by [`write`] — callers never set it, and it is empty on a record
    /// that has not been stored yet. It is what the category indexes hold, so a
    /// record can be pointed at from more than one table without being copied.
    #[serde(default)]
    pub event_id: String,
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

fn serialize(record: &AuditRecord) -> RedisResult<String> {
    serde_json::to_string(record).map_err(|e| {
        deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::TypeError,
            "chat: audit serialize",
            e.to_string(),
        ))
    })
}

/// Store a record once and index it under every table that should show it.
///
/// The record is written to its own key and only its **id** is pushed onto the
/// category indexes — so an action that is both a player action and a list edit
/// (a ban, an un-ban) appears in both tables while existing exactly once. That
/// is the whole point: two views, one truth, nothing to keep in step.
///
/// `category` names the record's home index. It additionally joins the list
/// index whenever [`AuditRecord::list`] is set; nothing writes to the list index
/// directly, because a second writer is exactly how the two tables would drift.
///
/// Failures are logged and swallowed by callers where the moderation action has
/// already happened — losing the log line is bad, but failing the action the
/// moderator just took (and leaving them unsure whether it applied) is worse.
pub async fn write(
    conn: &mut deadpool_redis::Connection,
    category: AuditCategory,
    record: &AuditRecord,
) -> RedisResult<()> {
    debug_assert_ne!(
        category,
        AuditCategory::List,
        "the list index is derived from the `list` tag — never a write target"
    );

    let event_id = ids::next_audit_id(conn).await?;
    let mut stored = record.clone();
    stored.event_id.clone_from(&event_id);
    let json = serialize(&stored)?;

    index_record(conn, category, &event_id, &json, &stored.list).await
}

/// Write one record key and push its id onto the indexes it belongs in.
///
/// Atomic, so a failure can never leave an index pointing at a record that was
/// not stored — a dangling pointer would render as a silently missing row.
async fn index_record(
    conn: &mut deadpool_redis::Connection,
    home: AuditCategory,
    event_id: &str,
    json: &str,
    list: &str,
) -> RedisResult<()> {
    let mut pipe = deadpool_redis::redis::pipe();
    pipe.atomic();
    pipe.set(record_key(event_id), json).ignore();
    pipe.lpush(home.key(), event_id).ignore();
    // Guarded on `home` as well as the tag so a list-tagged record whose home is
    // somehow already List cannot be indexed onto it twice.
    if !list.is_empty() && home != AuditCategory::List {
        pipe.lpush(AuditCategory::List.key(), event_id).ignore();
    }
    pipe.query_async(conn).await
}

/// Records at index positions `start..=stop` for a category, newest first.
///
/// Two round trips regardless of depth: `LRANGE` the index for the window, then
/// one `MGET` for exactly those records. Paging deep costs the same as paging
/// the first page, which is what makes an arbitrary range usable.
pub async fn read(
    conn: &mut deadpool_redis::Connection,
    category: AuditCategory,
    start: isize,
    stop: isize,
) -> RedisResult<Vec<AuditRecord>> {
    let ids: Vec<String> = conn.lrange(category.key(), start, stop).await?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<String> = ids.iter().map(|id| record_key(id)).collect();
    let raw: Vec<Option<String>> = conn.mget(&keys).await?;

    Ok(raw
        .into_iter()
        .zip(&ids)
        .filter_map(|(json, event_id)| {
            let Some(json) = json else {
                // The index outlived the record it names. Skipping keeps the
                // rest of the page readable, which matters more than the gap.
                tracing::warn!(%event_id, "chat: audit index points at a missing record");
                return None;
            };
            match serde_json::from_str::<AuditRecord>(&json) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!(%event_id, "chat: skipping malformed audit record: {}", e);
                    None
                }
            }
        })
        .collect())
}

/// Move pre-0.34.0 records into the record store and build the indexes.
///
/// Runs at boot before either server binds, so no request can observe a
/// half-migrated log. A no-op once [`MIGRATION_MARKER`] is set.
///
/// # Why it rebuilds rather than appends
///
/// The indexes are deleted and rebuilt from the legacy lists. That makes a
/// crashed run self-healing: the marker is only set after a complete pass, so
/// the next boot starts from scratch instead of appending a second copy of
/// whatever it already moved. Records orphaned by the failed run keep their own
/// keys and are simply never pointed at — wasted bytes, not corruption.
///
/// **Operator note:** clearing the marker by hand is therefore a *rebuild from
/// the legacy lists*, which discards anything written since the migration. It is
/// not a harmless re-run once the deployment has taken new actions.
///
/// The legacy lists are only ever read. A rollback to an older server finds its
/// data exactly as it left it.
pub async fn migrate(conn: &mut deadpool_redis::Connection) -> RedisResult<()> {
    if conn.exists(MIGRATION_MARKER).await? {
        return Ok(());
    }

    for category in AuditCategory::ALL {
        // Non-empty here means a previous run died partway, or someone cleared
        // the marker. Either way the rebuild is correct, but it is worth saying
        // out loud rather than silently discarding rows.
        let stale: isize = conn.llen(category.key()).await.unwrap_or(0);
        if stale > 0 {
            tracing::warn!(
                index = %category.key(),
                entries = stale,
                "chat: rebuilding a non-empty audit index — prior run incomplete or marker cleared"
            );
        }
        let _: () = conn.del(category.key()).await?;
    }

    for category in AuditCategory::ALL {
        // Stored newest-first, so replay in reverse to rebuild the same order.
        let legacy: Vec<String> = conn.lrange(category.legacy_key(), 0, -1).await?;
        let mut moved = 0usize;
        let mut skipped = 0usize;

        for json in legacy.iter().rev() {
            let Ok(mut record) = serde_json::from_str::<AuditRecord>(json) else {
                // A record that will not parse cannot be rendered either, so it
                // was already invisible. Counted, not fatal.
                skipped += 1;
                continue;
            };

            // Legacy List rows are un-bans — player actions that happen to edit
            // a list. Under the new model their home is the player index, and
            // the tag puts them back in the List view.
            let home = match category {
                AuditCategory::List => AuditCategory::Player,
                other => other,
            };
            let list = if category == AuditCategory::List && record.list.is_empty() {
                // Keep it visible in the table it came from even if untagged.
                record.list = "Ban List".to_string();
                record.list.clone()
            } else {
                record.list.clone()
            };

            let event_id = ids::next_audit_id(conn).await?;
            record.event_id.clone_from(&event_id);
            let encoded = serialize(&record)?;
            index_record(conn, home, &event_id, &encoded, &list).await?;
            moved += 1;
        }

        if moved > 0 || skipped > 0 {
            tracing::info!(
                from = %category.legacy_key(),
                moved,
                skipped,
                "chat: audit records migrated"
            );
        }
    }

    let _: () = conn.set(MIGRATION_MARKER, "1").await?;
    tracing::info!("chat: audit log migration complete");
    Ok(())
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
        let keys = AuditCategory::ALL.map(|c| c.key());
        assert_eq!(keys, [
            "chat:audit:idx:player",
            "chat:audit:idx:word",
            "chat:audit:idx:list",
            "chat:audit:idx:system",
        ]);
        // No index may shadow another's list.
        let mut sorted = keys;
        sorted.sort_unstable();
        sorted.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
    }

    /// The migration reads the legacy lists and writes the indexes. If any pair
    /// collided it would be reading its own output — appending to a list it is
    /// iterating, and duplicating every record it moved. Nothing else in the
    /// code would catch that, so it is asserted directly.
    #[test]
    fn index_keys_never_collide_with_legacy_keys() {
        for category in AuditCategory::ALL {
            for other in AuditCategory::ALL {
                assert_ne!(
                    category.key(),
                    other.legacy_key(),
                    "index {:?} collides with legacy {:?}",
                    category,
                    other
                );
            }
        }
    }

    /// The legacy names are the pre-0.34.0 on-disk contract. Changing one
    /// silently strands a deployment's existing records: the migration would
    /// find an empty list and report success.
    #[test]
    fn legacy_keys_are_the_pre_migration_names() {
        assert_eq!(AuditCategory::ALL.map(|c| c.legacy_key()), [
            "chat:audit:player",
            "chat:audit:word",
            "chat:audit:list",
            "chat:audit:system",
        ]);
    }

    /// Records live under their own prefix, distinct from every index — a record
    /// key colliding with an index key would make one overwrite the other.
    #[test]
    fn record_keys_sit_outside_the_indexes() {
        let key = record_key("a00000000001");
        assert_eq!(key, "chat:audit:rec:a00000000001");
        for category in AuditCategory::ALL {
            assert_ne!(key, category.key());
            assert_ne!(key, category.legacy_key());
            assert!(!key.starts_with(category.key()));
        }
    }

    /// `event_id` is assigned by `write`, so a freshly built record must not
    /// carry one — a caller-set id would be overwritten and the mismatch would
    /// only show up as an index pointing somewhere unexpected.
    #[test]
    fn a_fresh_record_has_no_event_id() {
        let r = AuditRecord::by_moderator("jeanluc", "sub", AdminRole::Admin, "Ban", "Slurs");
        assert!(r.event_id.is_empty());

        // And it survives a round trip once set, since the indexes hold it.
        let mut stored = r.clone();
        stored.event_id = "a00000000007".into();
        let back: AuditRecord =
            serde_json::from_str(&serde_json::to_string(&stored).unwrap()).unwrap();
        assert_eq!(back.event_id, "a00000000007");
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
