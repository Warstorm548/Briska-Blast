//! View models for the Chat-Mod panel — the plain data the handler layer
//! (`admin::chatmod_data`) projects Redis onto, and the renderers in the
//! sibling modules turn into HTML. No rendering logic lives here.

/// One preview line in a session card. `flagged_words` get the red highlight
/// so flags stay visible from every view — a moderator inside one session
/// still spots blacklisted words surfacing in the other sessions' previews.
pub struct PreviewLine {
    pub text: String,
    /// Every blacklisted word that fired on this line, not just the first —
    /// a line censored twice in-game must read as censored twice here.
    pub flagged_words: Vec<String>,
}

/// One entry in the left "Active Game Sessions" panel.
pub struct ChatSession {
    pub code: String,
    /// The last few chat lines, newest last — the card's preview text.
    pub preview: Vec<PreviewLine>,
    /// True when the session has system-flagged messages (red-dot badge).
    pub flagged: bool,
}

/// A flagged message body inside a [`FlaggedSession`].
pub struct FlaggedBody {
    /// Message-body identifier (server-assigned 12-char alphanumeric once
    /// wired; never shown to game players — moderation surfaces only).
    pub body_id: String,
    /// Sender's username, shown with their player id.
    pub username: String,
    /// Sender's numeric player id (the `/register`-issued counter number).
    /// Rendered in the canonical zero-padded form via
    /// [`PlayerId::from_counter`] (9-digit minimum). Moderation surfaces
    /// only — never rendered game-side.
    pub player_id: u64,
    pub body: String,
    /// The blacklisted words that tripped the flag (highlighted red).
    pub words: Vec<String>,
}

/// One session's flagged messages, shown as a card on the landing page.
pub struct FlaggedSession {
    pub code: String,
    pub bodies: Vec<FlaggedBody>,
}

/// One transcript row in the session view.
///
/// `Default` exists for test fixtures, which care about two or three fields at a
/// time. Production has exactly one construction site
/// ([`crate::admin::chatmod_data`]), which fills every field explicitly.
#[derive(Default)]
pub struct ChatMessage {
    pub body_id: String,
    pub username: String,
    /// Sender's numeric player id (the `/register`-issued counter number).
    /// Rendered in the canonical zero-padded form via
    /// [`PlayerId::from_counter`] (9-digit minimum). Moderation surfaces
    /// only — never rendered game-side.
    ///
    /// `None` for a moderator line: a moderator speaks through this panel and has
    /// no player account, so there is no id to show and no player to act against.
    /// Rendering a zero-padded `000000000` would assert a player that isn't there.
    pub player_id: Option<u64>,
    pub body: String,
    /// Every blacklisted word the body contains, to highlight. Empty when the
    /// line never tripped the filter.
    pub flagged_words: Vec<String>,
    /// True when a moderator spoke into the session rather than a player. Drives
    /// the `MOD` tag so their line is never mistaken for a player's.
    pub is_moderator: bool,
    /// For a moderator line posted anonymously: the name **players** saw, i.e.
    /// the generic `Mod`. `None` when they posted under their own name.
    ///
    /// `username` always holds the real moderator either way. Anonymity is
    /// directed at players, never at the moderation team — with several
    /// moderators working one session, "who said this" has to stay answerable
    /// from the transcript, and it is recorded on every line regardless.
    pub posted_as: Option<String>,
    /// True when this line is a warning a moderator sent to one player, rather
    /// than anything said in the chat. Mutually exclusive with `is_moderator`;
    /// [`message_role`] is the single place that decides between them.
    ///
    /// `username`/`player_id` name the **target**, and `body` is the reason.
    pub is_warning: bool,
    /// True when this line is a chat ban a moderator applied to one player. Same
    /// field roles as `is_warning` — target in `username`/`player_id`, reason in
    /// `body` — and likewise decided only through [`message_role`].
    pub is_ban: bool,
    /// For a warning or a ban: whether it actually reached the target's client.
    /// `None` on every other kind, where delivery is not a concept.
    pub delivered: Option<bool>,
    /// Set when this body has been withdrawn from players' view. The message
    /// still renders — the moderator's copy is the record — but greyed, with a
    /// note saying it is no longer visible to players.
    pub deleted: Option<Deletion>,
}

/// Who withdrew a body from players' view, and why.
#[derive(Default)]
pub struct Deletion {
    /// The acting moderator's real display name. A deletion is never anonymous.
    pub mod_user: String,
    pub reason: String,
    /// Pre-formatted UTC timestamp, matching the audit tables' rendering.
    pub at: String,
}

/// What a transcript line is, for the one branch that picks its tag and styling.
///
/// The view model carries booleans because that is how each fact arrives from
/// storage, but nothing should read them individually — collapsing them here
/// means a line can never render as two things at once.
#[derive(Clone, Copy, PartialEq)]
pub enum MessageRole {
    Player,
    Moderator,
    Warning,
    Ban,
}

/// Resolve a line's role. A ban or a warning wins over the moderator flag: both
/// are sent *by* a moderator, so both facts are true of them, but only one
/// describes the line. Ban is checked first — it is the stronger action, and the
/// two are never set together.
pub fn message_role(m: &ChatMessage) -> MessageRole {
    if m.is_ban {
        MessageRole::Ban
    } else if m.is_warning {
        MessageRole::Warning
    } else if m.is_moderator {
        MessageRole::Moderator
    } else {
        MessageRole::Player
    }
}

/// The Chat Audit Logs are split into category tables (chosen by a dropdown),
/// each with its own direct headers. Every category shares the same leading
/// "who / when / what / why" columns — Timestamp, Display Name, Group, Action,
/// Reason — then adds its own subject/evidence columns. `AuditLog` groups the
/// four category records — players, words, lists, and system — so a handler can
/// pass all of them to the page.
///
/// (The admin-panel "Access" log — role/setting changes — deliberately lives
/// elsewhere; it does not pertain to chat moderation.)
///
/// Automated (program-initiated) actions carry `Group = "System"`. Those that
/// enforce on a player (auto-delete/suspend/ban) land in `players` like any
/// other player action; the `system` table is only for automated events that
/// aren't a Player/Word/List enforcement — chiefly word flagging.
pub struct AuditLog {
    pub players: Vec<PlayerAuditEntry>,
    pub words: Vec<WordAuditEntry>,
    pub lists: Vec<ListAuditEntry>,
    pub system: Vec<SystemAuditEntry>,
    /// Which slice of each log these rows came from, e.g. `100-200`.
    ///
    /// Rendered on the page even when rows come back, because every table here
    /// is a *window* — an empty one means "nothing in the range you asked for",
    /// which reads as "this never happened" unless the range is on screen.
    pub window_label: String,
    /// Set when the requested range could not be honoured as typed (rejected or
    /// clamped), so a moderator is never silently shown a different slice than
    /// the one they asked for.
    pub window_notice: Option<String>,
}

/// **Player** category — an action taken on a player's chat privileges. One
/// record per (action instance, target player); a bulk action spanning several
/// players splits into one record per player, and repeated actions on the same
/// player are never merged. See the audit-log contract in `admin::chatmod`.
pub struct PlayerAuditEntry {
    /// When the action was taken, shown at the start of the row.
    pub timestamp: String,
    /// The acting moderator's display name.
    pub moderator_display: String,
    /// The acting moderator's role/group (e.g. `Admin`, `Moderator`).
    pub moderator_group: String,
    /// The action taken (e.g. `Ban`, `Suspend 1d`, `Warn + Delete`).
    pub action: String,
    /// Reason recorded with the action (audit-logged, may be player-facing).
    pub reason: String,
    /// Target player's username.
    pub target_username: String,
    /// Target player's numeric id, rendered zero-padded via
    /// [`PlayerId::from_counter`]. Moderation surfaces only.
    pub target_player_id: u64,
    /// Identifiers of the message bodies this action covered — any tool may act
    /// on zero, one, or several of the player's bodies at once. Rendered as the
    /// single id, an em-dash when body-less, or a `×N bodies` disclosure.
    pub body_ids: Vec<String>,
    /// Blacklisted word(s) tied to the record; rendered as red chips (em-dash
    /// when empty).
    pub flagged_words: Vec<String>,
    /// Snapshot of the chat as it stood when the action was taken — surfaced
    /// read-only in the Transcript overlay.
    pub snapshot: Vec<ChatMessage>,
    /// For a ban, whose snapshot is the *whole* transcript: how many lines
    /// preceded the action, so the overlay can divide what prompted it from what
    /// followed. `None` on every record whose snapshot already ends at the
    /// action — there is nothing after it to mark off.
    pub snapshot_cut: Option<usize>,
}

/// **Word** category — blacklist/approve actions on a word. Approve targets a
/// specific occurrence (so it carries the sender + body + snapshot); Blacklist
/// is a global add with no player/body.
pub struct WordAuditEntry {
    pub timestamp: String,
    pub moderator_display: String,
    pub moderator_group: String,
    /// `Blacklist Word` or `Approve Word`.
    pub action: String,
    pub reason: String,
    /// The word the action was about.
    pub word: String,
    /// The occurrence's sender (Approve only); `None` for a global Blacklist.
    pub target_username: Option<String>,
    pub target_player_id: Option<u64>,
    /// The approved occurrence's bodies; empty for a global Blacklist.
    pub body_ids: Vec<String>,
    /// Chat snapshot; empty ⇒ the row's Transcript cell is a plain dash.
    pub snapshot: Vec<ChatMessage>,
}

/// **List** category — moderation-list edits (un-ban, lift suspension, whitelist
/// changes). Targets a player and names which list; no chat snapshot.
pub struct ListAuditEntry {
    pub timestamp: String,
    pub moderator_display: String,
    pub moderator_group: String,
    /// `Remove Ban`, `Lift Suspension`, `Whitelist Add`, `Whitelist Remove`.
    pub action: String,
    pub reason: String,
    pub target_username: String,
    pub target_player_id: u64,
    /// Which list was edited: `Ban List`, `Suspensions`, `Whitelist`.
    pub list: String,
}

/// **System** category — an automated event that isn't a direct enforcement on a
/// player (auto-enforcement lands in `PlayerAuditEntry` with `Group = System`).
/// Chiefly word flagging: the system surfacing a blacklisted word to moderators.
/// `Group` is always `System`, so the field is implied rather than stored.
pub struct SystemAuditEntry {
    pub timestamp: String,
    /// The automated process (the Display Name slot): `Word Filter`, `Auto-Mod`.
    pub source: String,
    /// `Flag Word`, and future non-enforcement automated actions.
    pub action: String,
    /// The rule/condition that fired (the Reason slot): `Matched blacklist`.
    pub trigger: String,
    /// The word the event was about.
    pub word: String,
    /// The player whose message the event references.
    pub target_username: String,
    pub target_player_id: u64,
    /// The referenced message bodies.
    pub body_ids: Vec<String>,
    /// Chat snapshot for the Transcript overlay.
    pub snapshot: Vec<ChatMessage>,
}

/// One blacklisted word in the **Backlisted Words** sub-tab's "Words In List"
/// table. `active_filter` mirrors the Active Filter Toggle checkbox — a word can
/// stay on the list yet be temporarily disabled from filtering.
pub struct BlacklistWord {
    pub word: String,
    /// Reason recorded when the word was blacklisted.
    pub reason: String,
    /// True when the word is currently enforced by the filter.
    pub active_filter: bool,
}

/// One row in the **Banned Users** sub-tab table: a player barred from chat.
pub struct BannedUser {
    pub timestamp: String,
    pub username: String,
    /// The player's numeric id, rendered zero-padded via
    /// [`PlayerId::from_counter`]. Moderation surfaces only.
    pub player_id: u64,
    pub reason: String,
    /// The conversation the ban happened in, in **full** — a ban is the one
    /// action whose record keeps the whole transcript rather than cutting at the
    /// moment it was taken. Empty for a ban applied from this page, which has no
    /// session context; the Transcript cell then shows an em-dash.
    pub snapshot: Vec<ChatMessage>,
    /// How many lines preceded the ban, dividing what prompted it from what
    /// followed. See [`PlayerAuditEntry::snapshot_cut`].
    pub snapshot_cut: Option<usize>,
}

impl BannedUser {
    /// Whether there is a chat to open from the Transcript cell. Derived rather
    /// than stored so the cell can never advertise an overlay that renders empty.
    pub fn has_transcript(&self) -> bool {
        !self.snapshot.is_empty()
    }
}

/// One row in the **Active Suspensions** sub-tab table: a temporary chat mute.
pub struct SuspendedUser {
    pub timestamp: String,
    pub username: String,
    pub player_id: u64,
    /// Total suspension length, e.g. `1d 6h`.
    pub suspended_for: String,
    /// Time left before it lifts, e.g. `18h 42m`.
    pub remaining: String,
    pub reason: String,
}

/// The datasets behind the Moderation Lists page's sub-tabs. Whitelisted Users
/// has no mockup yet, so it carries no data (placeholder tab).
pub struct ModerationLists {
    pub blacklist: Vec<BlacklistWord>,
    pub banned: Vec<BannedUser>,
    pub suspended: Vec<SuspendedUser>,
}
