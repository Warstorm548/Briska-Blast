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
    blacklist,
    transcript::{self, MessageKind, StoredMessage},
};
use crate::state::AppState;
use super::templates::{
    AuditLog, BannedUser, BlacklistWord, ChatMessage, ChatSession, FlaggedBody, FlaggedSession,
    ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
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

fn to_view(msg: &StoredMessage) -> ChatMessage {
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
    transcript::all(conn, &sid)
        .await
        .unwrap_or_default()
        .iter()
        .map(to_view)
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
/// Only the blacklist is wired. Banned and suspended are empty because no tool
/// writes them yet — showing invented rows in a panel that is otherwise live
/// would read as real moderation history.
pub async fn moderation_lists(state: &AppState) -> ModerationLists {
    let blacklist = match state.redis.get().await {
        Ok(mut conn) => blacklist::load(&mut conn)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|e| BlacklistWord {
                word: e.word,
                reason: e.reason,
                active_filter: e.active,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    ModerationLists {
        blacklist,
        // Named explicitly rather than inferred: these stay empty until a ban or
        // suspend tool exists to write them, and the types say what will fill them.
        banned: Vec::<BannedUser>::new(),
        suspended: Vec::<SuspendedUser>::new(),
    }
}

/// Reads transcripts once per instance, so a page of records that all point at
/// one session doesn't re-read it for every row.
#[derive(Default)]
struct SnapshotCache {
    by_sid: HashMap<String, Vec<StoredMessage>>,
}

impl SnapshotCache {
    /// The chat as it stood when a record was written: the first `cut_index`
    /// lines of its transcript. This is what "a snapshot of the entire chat"
    /// resolves to — pinned, not copied.
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
        }
        let lines = &self.by_sid[&record.sid];
        let cut = record.cut_index.min(lines.len());
        lines[..cut].iter().map(to_view).collect()
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
        players.push(PlayerAuditEntry {
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

    let lists = read(&mut conn, AuditCategory::List)
        .await
        .into_iter()
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
        .collect();

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

async fn read(conn: &mut deadpool_redis::Connection, category: AuditCategory) -> Vec<AuditRecord> {
    audit::read(conn, category, AUDIT_PAGE_LIMIT)
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
        let view = to_view(&line(MessageKind::Player, "Warstorm", "000000007", "nice shot"));
        assert_eq!(view.username, "Warstorm");
        assert_eq!(view.player_id, Some(7));
        assert!(!view.is_moderator);
    }

    #[test]
    fn moderator_line_has_no_player_id() {
        // A moderator has no player account; surfacing a zero id would assert one.
        let mut msg = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        msg.mod_user = "jeanluc".into();
        let view = to_view(&msg);
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
        let view = to_view(&msg);
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

        let a = to_view(&first);
        let b = to_view(&second);
        assert_ne!(a.username, b.username, "both would otherwise read as 'Mod'");
        assert_eq!(a.posted_as, b.posted_as, "players saw the same label for both");
    }

    #[test]
    fn named_moderator_carries_no_posted_as_suffix() {
        let mut msg = line(MessageKind::Moderator, "jeanluc", "", "keep it civil");
        msg.mod_user = "jeanluc".into();
        msg.mod_anonymous = false;
        let view = to_view(&msg);
        assert_eq!(view.username, "jeanluc");
        assert_eq!(view.posted_as, None, "nothing to disclose — they used their name");
    }

    #[test]
    fn moderator_line_without_a_recorded_identity_falls_back() {
        // Defensive: a record written before the identity was captured, or one
        // whose write partially failed, must not render as an unattributed blank.
        let msg = line(MessageKind::Moderator, "Mod", "", "keep it civil");
        let view = to_view(&msg);
        assert_eq!(view.username, "Mod");
        assert_eq!(view.posted_as, None);
    }

    #[test]
    fn player_lines_never_carry_an_attribution_suffix() {
        let view = to_view(&line(MessageKind::Player, "Warstorm", "000000007", "hi"));
        assert_eq!(view.posted_as, None);
    }

    #[test]
    fn missing_username_falls_back_to_the_player_id_form() {
        let view = to_view(&line(MessageKind::Player, "", "000000007", "hi"));
        assert_eq!(view.username, "Player 000000007");
    }

    #[test]
    fn unparseable_player_id_degrades_without_panicking() {
        let view = to_view(&line(MessageKind::Player, "", "not-a-number", "hi"));
        assert_eq!(view.username, "Player");
        assert_eq!(view.player_id, None);
    }

    #[test]
    fn view_carries_the_uncensored_original() {
        let mut msg = line(MessageKind::Player, "Warstorm", "000000007", "frick you all");
        msg.flagged_words = vec!["frick".into()];
        let view = to_view(&msg);
        // Players received "##### you all"; the moderator must see what was typed.
        assert_eq!(view.body, "frick you all");
        assert_eq!(view.flagged_word.as_deref(), Some("frick"));
    }
}
