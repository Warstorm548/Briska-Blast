//! Live data behind the Chat-Mod pages.
//!
//! Projects the storage in [`crate::chat`] onto the view models the templates in
//! [`super::templates`] consume. Kept out of `super::chatmod` so the handlers stay
//! thin — they resolve a session, call one reader here, and render.
//!
//! # Scope
//!
//! The landing page shows **live** sessions. Retained transcripts from sessions
//! that have already ended are not browsable here yet: their evidence surfaces
//! through the Chat Audit Logs, where every flag wrote a record with a snapshot
//! pinned to it. A dedicated archive browser is a follow-up.
//!
//! Moderator lines are deliberately **not** run through the word filter. A
//! moderator quoting a slur in order to moderate it must not have their own
//! message censored, and they are trusted staff by definition.

use std::collections::HashMap;

use shared::types::player::PlayerId;

use crate::chat::{
    audit::{self, AuditCategory, AuditRecord},
    bans, blacklist,
    transcript::{self, DeletionMark, MessageKind, StoredMessage},
};
use crate::state::AppState;
use super::templates::{
    AuditLog, BannedUser, BlacklistWord, ChatMessage, ChatSession, Deletion, FlaggedBody,
    FlaggedSession, ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
    SystemAuditEntry, WordAuditEntry,
};

/// How many recent lines each session card previews.
const PREVIEW_LINES: isize = 3;

/// How many records per audit category a page load reads.
const AUDIT_PAGE_LIMIT: isize = 100;

/// Display name for a stored line, falling back to the same `Player <id>` shape
/// the game client uses when no username is on file.
fn display_name(msg: &StoredMessage) -> String {
    if !msg.username.is_empty() {
        return msg.username.clone();
    }
    match msg.player_id.parse::<u64>() {
        Ok(n) => format!("Player {}", PlayerId::from_counter(n)),
        Err(_) => "Player".to_string(),
    }
}

/// The numeric player id for moderation surfaces. `None` for moderator lines,
/// and for anything whose id doesn't parse.
fn numeric_player_id(msg: &StoredMessage) -> Option<u64> {
    if msg.kind == MessageKind::Moderator {
        return None;
    }
    msg.player_id.parse::<u64>().ok()
}

/// How a moderator line is attributed on the moderation surface.
///
/// Returns `(name_to_show, posted_as)`. The panel always names the **real**
/// moderator; `posted_as` records what players saw when the post was anonymous.
/// Anonymity is directed at players, not at the moderation team — with several
/// moderators working one session, two anonymous lines both reading `Mod` would
/// leave colleagues (and later reviewers) unable to tell who said what, and the
/// identity is on every record anyway.
///
/// Falls back to the broadcast name if `mod_user` is somehow empty, so a line
/// never renders anonymous-looking by accident.
fn moderator_attribution(msg: &StoredMessage) -> (String, Option<String>) {
    let real = if msg.mod_user.is_empty() {
        msg.username.clone()
    } else {
        msg.mod_user.clone()
    };
    let posted_as = (msg.mod_anonymous && real != msg.username).then(|| msg.username.clone());
    (real, posted_as)
}

/// Project a stored line for rendering.
///
/// `deleted` is the instance's deletion marks, keyed by body id — passed in
/// rather than looked up per line so one `HGETALL` covers a whole transcript.
/// Empty when nothing in the session has been withdrawn.
fn to_view(msg: &StoredMessage, deleted: &HashMap<String, DeletionMark>) -> ChatMessage {
    // A warning's `username`/`player_id` name the target, not the moderator, so
    // it takes the ordinary display path — only a moderator *speaking* is
    // re-attributed.
    let (username, posted_as) = if msg.kind == MessageKind::Moderator {
        moderator_attribution(msg)
    } else {
        (display_name(msg), None)
    };
    ChatMessage {
        body_id: msg.body_id.clone(),
        username,
        player_id: numeric_player_id(msg),
        // The transcript keeps the uncensored original — that is the whole point
        // of capturing it. Players received the masked form.
        body: msg.text.clone(),
        flagged_word: msg.flagged_words.first().cloned(),
        is_moderator: msg.kind == MessageKind::Moderator,
        posted_as,
        is_warning: msg.kind == MessageKind::Warning,
        is_ban: msg.kind == MessageKind::Ban,
        delivered: msg.delivered,
        deleted: deleted.get(&msg.body_id).map(|d| Deletion {
            mod_user: d.mod_user.clone(),
            reason: d.reason.clone(),
            at: audit::format_timestamp(d.at_ms),
        }),
    }
}

/// Codes of every live session, sorted for a stable panel order.
async fn live_codes(conn: &mut deadpool_redis::Connection) -> Vec<String> {
    use deadpool_redis::redis::AsyncCommands;
    let keys: Vec<String> = conn.keys("session:*").await.unwrap_or_default();
    let mut codes: Vec<String> = keys
        .iter()
        .filter_map(|k| k.strip_prefix("session:").map(str::to_string))
        .collect();
    codes.sort();
    codes
}

/// Uppercase a candidate code, rejecting anything that isn't a plausible one.
///
/// Split out from [`resolve_live_session`] so the shape guard is testable without
/// a live Redis, and so junk never reaches a key lookup. Session codes are 6
/// chars from `api::host::CODE_ALPHABET`; the bound here is deliberately looser
/// than that so a future code-length change doesn't silently start rejecting.
fn canonical_code(code: &str) -> Option<String> {
    let canon = code.to_ascii_uppercase();
    let plausible = !canon.is_empty()
        && canon.len() <= 12
        && canon.chars().all(|c| c.is_ascii_alphanumeric());
    plausible.then_some(canon)
}

/// Resolve a user-supplied code to a live session, canonicalised to uppercase.
///
/// Returns `None` for anything that isn't currently live, which is what keeps a
/// hostile `?from=` value from being reflected back into a link. This replaces
/// the sample-list lookup and preserves its contract exactly.
pub async fn resolve_live_session(state: &AppState, code: &str) -> Option<String> {
    use deadpool_redis::redis::AsyncCommands;
    let canon = canonical_code(code)?;
    let mut conn = state.redis.get().await.ok()?;
    let exists: bool = conn
        .exists(format!("session:{canon}"))
        .await
        .unwrap_or(false);
    exists.then_some(canon)
}

/// The left "Active Game Sessions" panel, from an enumeration the caller already
/// made. Split from [`live_sessions`] so a page needing several views pays for
/// one connection and one `KEYS` scan rather than one of each per view.
async fn collect_sessions(
    conn: &mut deadpool_redis::Connection,
    codes: &[String],
) -> Vec<ChatSession> {
    let mut out = Vec::with_capacity(codes.len());
    for code in codes.iter().cloned() {
        let sid = transcript::live_sid(conn, &code).await.ok().flatten();
        let (preview, flagged) = match sid {
            Some(sid) => {
                let lines = transcript::tail(conn, &sid, PREVIEW_LINES)
                    .await
                    .unwrap_or_default();
                let preview = lines
                    .iter()
                    .map(|m| PreviewLine {
                        text: format!("{}: {}", display_name(m), m.text),
                        flagged_word: m.flagged_words.first().cloned(),
                    })
                    .collect();
                let flagged = transcript::is_flagged(conn, &sid).await.unwrap_or(false);
                (preview, flagged)
            }
            // A session with no chat yet still belongs in the panel — a moderator
            // may want to enter it before anyone has spoken.
            None => (Vec::new(), false),
        };
        out.push(ChatSession { code, preview, flagged });
    }
    out
}

/// The landing page's "Flagged Messages" panel, from a caller-supplied
/// enumeration (see [`collect_sessions`] for why).
async fn collect_flagged(
    conn: &mut deadpool_redis::Connection,
    codes: &[String],
) -> Vec<FlaggedSession> {
    let mut out = Vec::new();
    for code in codes.iter().cloned() {
        let Ok(Some(sid)) = transcript::live_sid(conn, &code).await else {
            continue;
        };
        if !transcript::is_flagged(conn, &sid).await.unwrap_or(false) {
            continue;
        }
        let bodies: Vec<FlaggedBody> = transcript::all(conn, &sid)
            .await
            .unwrap_or_default()
            .iter()
            .filter(|m| m.is_flagged())
            .filter_map(|m| {
                Some(FlaggedBody {
                    body_id: m.body_id.clone(),
                    username: display_name(m),
                    player_id: numeric_player_id(m)?,
                    body: m.text.clone(),
                    word: m.flagged_words.first().cloned().unwrap_or_default(),
                })
            })
            .collect();
        if !bodies.is_empty() {
            out.push(FlaggedSession { code, bodies });
        }
    }
    out
}

/// A live session's transcript, oldest first.
async fn collect_transcript(
    conn: &mut deadpool_redis::Connection,
    code: &str,
) -> Vec<ChatMessage> {
    let Ok(Some(sid)) = transcript::live_sid(conn, code).await else {
        // No chat in this session yet — the template renders its own empty state.
        return Vec::new();
    };
    // One read for the whole transcript. A failure here greys nothing rather
    // than hiding the messages — the moderator still sees the conversation.
    let deleted = transcript::deletions(conn, &sid).await.unwrap_or_default();
    transcript::all(conn, &sid)
        .await
        .unwrap_or_default()
        .iter()
        .map(|m| to_view(m, &deleted))
        .collect()
}

/// The left panel alone — for the two Chat Nav sub-pages, which show sessions
/// but neither the flagged panel nor a transcript.
pub async fn live_sessions(state: &AppState) -> Vec<ChatSession> {
    let Ok(mut conn) = state.redis.get().await else {
        return Vec::new();
    };
    let codes = live_codes(&mut conn).await;
    collect_sessions(&mut conn, &codes).await
}

/// Both landing-page panels: session cards and the flagged overview.
///
/// One connection and one `KEYS session:*` scan for the pair. Fetched separately
/// they cost two of each on every 5s poll, for two views of the same enumeration.
pub async fn landing_view(state: &AppState) -> (Vec<ChatSession>, Vec<FlaggedSession>) {
    let Ok(mut conn) = state.redis.get().await else {
        return (Vec::new(), Vec::new());
    };
    let codes = live_codes(&mut conn).await;
    let sessions = collect_sessions(&mut conn, &codes).await;
    let flagged = collect_flagged(&mut conn, &codes).await;
    (sessions, flagged)
}

/// The entered-session view: the left panel plus that session's transcript.
///
/// Shares one connection, which matters more here than on the landing page —
/// this is the 2s poll.
pub async fn session_view(state: &AppState, code: &str) -> (Vec<ChatSession>, Vec<ChatMessage>) {
    let Ok(mut conn) = state.redis.get().await else {
        return (Vec::new(), Vec::new());
    };
    let codes = live_codes(&mut conn).await;
    let sessions = collect_sessions(&mut conn, &codes).await;
    let transcript = collect_transcript(&mut conn, code).await;
    (sessions, transcript)
}

/// The Moderation Lists datasets.
///
/// Suspensions stay empty because no tool writes them yet — showing invented rows
/// in a panel that is otherwise live would read as real moderation history.
pub async fn moderation_lists(state: &AppState) -> ModerationLists {
    let Ok(mut conn) = state.redis.get().await else {
        return ModerationLists {
            blacklist: Vec::new(),
            banned: Vec::new(),
            suspended: Vec::new(),
        };
    };

    let blacklist = blacklist::load(&mut conn)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|e| BlacklistWord {
            word: e.word,
            reason: e.reason,
            active_filter: e.active,
        })
        .collect();

    // Ban rows replay their transcript through the same cache the audit page
    // uses, so several bans from one session read it once. A ban stores the whole
    // conversation rather than a cut, which is what `full` asks for here.
    let mut cache = SnapshotCache::default();
    let mut banned = Vec::new();
    for entry in bans::load(&mut conn).await.unwrap_or_default() {
        let pinned = AuditRecord {
            sid: entry.sid.clone(),
            cut_index: entry.cut_index,
            full: true,
            ..Default::default()
        };
        let snapshot = cache.snapshot(&mut conn, &pinned).await;
        banned.push(BannedUser {
            timestamp: audit::format_timestamp(entry.at_ms),
            username: entry.username,
            player_id: entry.player_id,
            reason: entry.reason,
            snapshot_cut: SnapshotCache::cut_marker(&pinned, snapshot.len()),
            snapshot,
        });
    }

    ModerationLists {
        blacklist,
        banned,
        // Named explicitly rather than inferred: this stays empty until a suspend
        // tool exists to write it, and the type says what will fill it.
        suspended: Vec::<SuspendedUser>::new(),
    }
}

/// Reads transcripts once per instance, so a page of records that all point at
/// one session doesn't re-read it for every row.
#[derive(Default)]
struct SnapshotCache {
    by_sid: HashMap<String, Vec<StoredMessage>>,
    deleted_by_sid: HashMap<String, HashMap<String, DeletionMark>>,
}

impl SnapshotCache {
    /// The chat as it stood when a record was written: the first `cut_index`
    /// lines of its transcript. This is what "a snapshot of the entire chat"
    /// resolves to — pinned, not copied.
    ///
    /// A record with `full` set (bans) reads the **whole** transcript instead,
    /// including anything said after the action. `cut_index` then marks where the
    /// action fell rather than ending the view. Same storage either way — only
    /// the range differs.
    ///
    /// Deletion marks are applied as they stand *now*, not as of the cut. A body
    /// withdrawn after this record was written therefore shows as deleted here
    /// too. That is deliberate: the marks carry their own moderator and
    /// timestamp, so the reviewer can see it happened later, and the alternative
    /// — replaying a body as live when it has since been removed — would misread
    /// as evidence that is still visible to players.
    async fn snapshot(
        &mut self,
        conn: &mut deadpool_redis::Connection,
        record: &AuditRecord,
    ) -> Vec<ChatMessage> {
        if record.sid.is_empty() {
            return Vec::new();
        }
        if !self.by_sid.contains_key(&record.sid) {
            let lines = transcript::all(conn, &record.sid).await.unwrap_or_default();
            self.by_sid.insert(record.sid.clone(), lines);
            let deleted = transcript::deletions(conn, &record.sid)
                .await
                .unwrap_or_default();
            self.deleted_by_sid.insert(record.sid.clone(), deleted);
        }
        let lines = &self.by_sid[&record.sid];
        let deleted = &self.deleted_by_sid[&record.sid];
        let cut = if record.full {
            lines.len()
        } else {
            record.cut_index.min(lines.len())
        };
        lines[..cut].iter().map(|m| to_view(m, deleted)).collect()
    }

    /// Where in a rendered snapshot the action fell, for the records that show
    /// the whole transcript. `None` when the snapshot already ends at the action,
    /// which is every other record — there is nothing after it to divide off.
    fn cut_marker(record: &AuditRecord, snapshot_len: usize) -> Option<usize> {
        (record.full && record.cut_index < snapshot_len).then_some(record.cut_index)
    }
}

/// All four audit category tables.
pub async fn audit_log(state: &AppState) -> AuditLog {
    let Ok(mut conn) = state.redis.get().await else {
        return AuditLog {
            players: Vec::new(),
            words: Vec::new(),
            lists: Vec::new(),
            system: Vec::new(),
        };
    };

    let mut cache = SnapshotCache::default();

    let mut players = Vec::new();
    for r in read(&mut conn, AuditCategory::Player).await {
        let snapshot = cache.snapshot(&mut conn, &r).await;
        let snapshot_cut = SnapshotCache::cut_marker(&r, snapshot.len());
        players.push(PlayerAuditEntry {
            snapshot_cut,
            timestamp: audit::format_timestamp(r.at_ms),
            moderator_display: r.actor.clone(),
            moderator_group: r.group.clone(),
            action: r.action.clone(),
            reason: r.reason.clone(),
            target_username: r.target_username.clone(),
            target_player_id: r.target_player_id.unwrap_or_default(),
            body_ids: r.body_ids.clone(),
            flagged_words: r.words.clone(),
            snapshot,
        });
    }

    let mut words = Vec::new();
    for r in read(&mut conn, AuditCategory::Word).await {
        let snapshot = cache.snapshot(&mut conn, &r).await;
        words.push(WordAuditEntry {
            timestamp: audit::format_timestamp(r.at_ms),
            moderator_display: r.actor.clone(),
            moderator_group: r.group.clone(),
            action: r.action.clone(),
            reason: r.reason.clone(),
            word: r.words.first().cloned().unwrap_or_default(),
            target_username: (!r.target_username.is_empty()).then(|| r.target_username.clone()),
            target_player_id: r.target_player_id,
            body_ids: r.body_ids.clone(),
            snapshot,
        });
    }

    let lists = list_entries(read(&mut conn, AuditCategory::List).await);

    let mut system = Vec::new();
    for r in read(&mut conn, AuditCategory::System).await {
        let snapshot = cache.snapshot(&mut conn, &r).await;
        system.push(SystemAuditEntry {
            timestamp: audit::format_timestamp(r.at_ms),
            source: r.actor.clone(),
            action: r.action.clone(),
            trigger: r.reason.clone(),
            word: r.words.first().cloned().unwrap_or_default(),
            target_username: r.target_username.clone(),
            target_player_id: r.target_player_id.unwrap_or_default(),
            body_ids: r.body_ids.clone(),
            snapshot,
        });
    }

    AuditLog { players, words, lists, system }
}

/// Project audit records into List-table rows.
///
/// The List table is a **view**, not a store: a record belongs in it when it
/// carries a [`AuditRecord::list`] tag, whatever category it is filed under. A
/// ban is filed under Player and shows here too; a warning is filed under Player
/// and does not.
///
/// The tag is re-checked even though the list index is built from it. The index
/// is only as correct as whatever wrote it, and an untagged row reaching the
/// table would render with an empty List column — a row that looks like it
/// edited nothing. Filtering here means the rendered table always matches the
/// stated rule.
///
/// Kept free of Redis so the rule itself is unit-testable; every other audit
/// projection is exercised only through the rendered page.
fn list_entries(records: Vec<AuditRecord>) -> Vec<ListAuditEntry> {
    records
        .into_iter()
        .filter(|r| !r.list.is_empty())
        .map(|r| ListAuditEntry {
            timestamp: audit::format_timestamp(r.at_ms),
            moderator_display: r.actor,
            moderator_group: r.group,
            action: r.action,
            reason: r.reason,
            target_username: r.target_username,
            target_player_id: r.target_player_id.unwrap_or_default(),
            list: r.list,
        })
        .collect()
}

async fn read(conn: &mut deadpool_redis::Connection, category: AuditCategory) -> Vec<AuditRecord> {
    audit::read(conn, category, 0, AUDIT_PAGE_LIMIT - 1)
        .await
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: MessageKind, username: &str, player_id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            body_id: "a00000000001".into(),
            kind,
            player_id: player_id.into(),
            username: username.into(),
            text: text.into(),
            flagged_words: vec![],
            mod_user: String::new(),
            mod_sub: String::new(),
            mod_anonymous: false,
            delivered: None,
            at_ms: 0,
        }
    }

    /// Project a line with nothing deleted — the ordinary case these tests are
    /// about. Deletion rendering has its own tests below.
    fn view(msg: &StoredMessage) -> ChatMessage {
        to_view(msg, &HashMap::new())
    }

    fn mark(reason: &str) -> DeletionMark {
        DeletionMark {
            mod_user: "jeanluc".into(),
            mod_sub: "pocket-id-sub-123".into(),
            reason: reason.into(),
            at_ms: 1_784_901_500_000,
        }
    }

    #[test]
    fn a_deleted_body_is_marked_but_still_projects_its_text() {
        // The moderator's copy is the record. Hiding it here would destroy the
        // evidence the deletion exists to preserve.
        let msg = line(MessageKind::Player, "Warstorm", "000000007", "frick you all");
        let deleted = HashMap::from([(msg.body_id.clone(), mark("Offensive language"))]);

        let v = to_view(&msg, &deleted);
        assert_eq!(v.body, "frick you all", "the transcript keeps the original");
        let d = v.deleted.expect("the mark must reach the view");
        assert_eq!(d.mod_user, "jeanluc");
        assert_eq!(d.reason, "Offensive language");
        assert_eq!(d.at, "2026-07-24 13:58:20 UTC");
    }

    #[test]
    fn marks_apply_only_to_the_body_they_name() {
        let deleted_line = line(MessageKind::Player, "Warstorm", "000000007", "bad");
        let mut other = line(MessageKind::Player, "EldenFire", "000000012", "fine");
        other.body_id = "a00000000009".into();
        let deleted = HashMap::from([(deleted_line.body_id.clone(), mark("Spam"))]);

        assert!(to_view(&deleted_line, &deleted).deleted.is_some());
        assert!(
            to_view(&other, &deleted).deleted.is_none(),
            "an unrelated body must not grey out"
        );
    }

    #[test]
    fn a_warning_names_the_target_and_carries_its_delivery() {
        // The player slots hold the target, so the panel must show the warned
        // player — not the moderator who sent it.
        let mut msg = line(MessageKind::Warning, "Warstorm", "000000007", "Offensive language");
        msg.mod_user = "jeanluc".into();
        msg.delivered = Some(false);

        let v = view(&msg);
        assert!(v.is_warning);
        assert!(!v.is_moderator, "a warning is not a moderator chat line");
        assert_eq!(v.username, "Warstorm", "the target, not the actor");
        assert_eq!(v.player_id, Some(7), "and it stays actionable");
        assert_eq!(v.body, "Offensive language", "the body is the reason");
        assert_eq!(v.delivered, Some(false));
        assert_eq!(v.posted_as, None, "warnings are never anonymous");
    }

    #[test]
    fn delivery_is_absent_on_lines_that_send_nothing() {
        assert_eq!(
            view(&line(MessageKind::Player, "Warstorm", "000000007", "hi")).delivered,
            None
        );
    }

    #[test]
    fn codes_are_canonicalized_to_uppercase() {
        assert_eq!(canonical_code("fj5b3v").as_deref(), Some("FJ5B3V"));
        assert_eq!(canonical_code("FJ5B3V").as_deref(), Some("FJ5B3V"));
    }

    #[test]
    fn implausible_codes_never_reach_a_key_lookup() {
        assert_eq!(canonical_code(""), None);
        assert_eq!(canonical_code("FJ5B3V*"), None, "punctuation");
        assert_eq!(canonical_code("session:FJ5B3V"), None, "key injection attempt");
        assert_eq!(canonical_code("../admin"), None, "path traversal attempt");
        assert_eq!(canonical_code("FJ5B3VFJ5B3V1"), None, "over length");
        assert_eq!(canonical_code("FJ 5B3V"), None, "whitespace");
    }

    #[test]
    fn player_line_projects_its_id() {
        let view = view(&line(MessageKind::Player, "Warstorm", "000000007", "nice shot"));
        assert_eq!(view.username, "Warstorm");
        assert_eq!(view.player_id, Some(7));
        assert!(!view.is_moderator);
    }

    #[test]
    fn moderator_line_has_no_player_id() {
        // A moderator has no player account; surfacing a zero id would assert one.
        let mut msg = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        msg.mod_user = "jeanluc".into();
        let view = view(&msg);
        assert_eq!(view.player_id, None);
        assert!(view.is_moderator);
    }

    #[test]
    fn anonymous_moderator_is_named_on_the_moderation_surface() {
        // Anonymity points at players, not at the moderation team. Two moderators
        // both posting as "Mod" must still be distinguishable here, or nobody can
        // tell who said what in a session several of them are working.
        let mut msg = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        msg.mod_user = "jeanluc".into();
        msg.mod_anonymous = true;
        let view = view(&msg);
        assert_eq!(view.username, "jeanluc", "the panel names the real moderator");
        assert_eq!(
            view.posted_as.as_deref(),
            Some("Mod"),
            "and records how it appeared to players"
        );
    }

    #[test]
    fn two_anonymous_moderators_are_distinguishable() {
        let mut first = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        first.mod_user = "jeanluc".into();
        first.mod_anonymous = true;
        let mut second = line(MessageKind::Moderator, "Mod", "", "last warning");
        second.mod_user = "alice".into();
        second.mod_anonymous = true;

        let a = view(&first);
        let b = view(&second);
        assert_ne!(a.username, b.username, "both would otherwise read as 'Mod'");
        assert_eq!(a.posted_as, b.posted_as, "players saw the same label for both");
    }

    #[test]
    fn named_moderator_carries_no_posted_as_suffix() {
        let mut msg = line(MessageKind::Moderator, "jeanluc", "", "keep it civil");
        msg.mod_user = "jeanluc".into();
        msg.mod_anonymous = false;
        let view = view(&msg);
        assert_eq!(view.username, "jeanluc");
        assert_eq!(view.posted_as, None, "nothing to disclose — they used their name");
    }

    #[test]
    fn moderator_line_without_a_recorded_identity_falls_back() {
        // Defensive: a record written before the identity was captured, or one
        // whose write partially failed, must not render as an unattributed blank.
        let msg = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        let view = view(&msg);
        assert_eq!(view.username, "Mod");
        assert_eq!(view.posted_as, None);
    }

    #[test]
    fn player_lines_never_carry_an_attribution_suffix() {
        let view = view(&line(MessageKind::Player, "Warstorm", "000000007", "hi"));
        assert_eq!(view.posted_as, None);
    }

    #[test]
    fn missing_username_falls_back_to_the_player_id_form() {
        let view = view(&line(MessageKind::Player, "", "000000007", "hi"));
        assert_eq!(view.username, "Player 000000007");
    }

    #[test]
    fn unparseable_player_id_degrades_without_panicking() {
        let view = view(&line(MessageKind::Player, "", "not-a-number", "hi"));
        assert_eq!(view.username, "Player");
        assert_eq!(view.player_id, None);
    }

    #[test]
    fn view_carries_the_uncensored_original() {
        let mut msg = line(MessageKind::Player, "Warstorm", "000000007", "frick you all");
        msg.flagged_words = vec!["frick".into()];
        let view = view(&msg);
        // Players received "##### you all"; the moderator must see what was typed.
        assert_eq!(view.body, "frick you all");
        assert_eq!(view.flagged_word.as_deref(), Some("frick"));
    }

    fn audit_record(action: &str, list: &str) -> AuditRecord {
        let mut r = AuditRecord::by_moderator(
            "modtester",
            "sub",
            crate::admin::AdminRole::Moderator,
            action,
            "Slur spam",
        );
        r.list = list.to_string();
        r.target_username = "EldenFire".into();
        r.target_player_id = Some(12);
        r
    }

    /// The rule the whole single-record model rests on: the tag decides whether
    /// a record is a list edit, not which table it was filed under. A ban and an
    /// un-ban are both Player records, and both belong in the List view.
    #[test]
    fn the_list_tag_decides_what_the_list_view_shows() {
        let entries = list_entries(vec![
            audit_record("Ban", "Ban List"),
            audit_record("Warn", ""),
            audit_record("Remove Ban", "Ban List"),
        ]);

        let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(
            actions,
            ["Ban", "Remove Ban"],
            "an untagged warning edits no list and must not appear"
        );
        assert!(entries.iter().all(|e| e.list == "Ban List"));
    }

    /// A row whose List column would render empty is a row claiming to have
    /// edited nothing. The index is built from the tag, so this can only happen
    /// via a bug or a legacy row — either way the table must not show it.
    #[test]
    fn untagged_records_never_reach_the_list_table() {
        assert!(list_entries(vec![audit_record("Warn + Delete", "")]).is_empty());
    }

    /// Lists other than the ban list project identically — the projection is
    /// driven by the tag being present, never by its value. Suspensions and
    /// Whitelisted Users therefore need no code here when they are built.
    #[test]
    fn any_tagged_list_projects_the_same_way() {
        for name in ["Ban List", "Suspensions", "Whitelist"] {
            let entries = list_entries(vec![audit_record("Suspend", name)]);
            assert_eq!(entries.len(), 1, "{name} should project");
            assert_eq!(entries[0].list, name);
            assert_eq!(entries[0].target_player_id, 12);
        }
    }
}
