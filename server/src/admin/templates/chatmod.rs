//! The admin **Chat-Mod** tab — the chat-moderation panel (UI preview).
//!
//! Layout phase only: both views render placeholder data supplied by the
//! handler (`admin::chatmod`) so the design can be iterated in a browser before
//! any wiring. Two server-rendered views share one full-width three-column
//! shell — the landing page (flagged-message overview) and the session view
//! (transcript + quick-access tools). This is the first tab to span the full
//! viewport width instead of the shared `.page` container; everything here is
//! `cm-`-prefixed so it can't collide with the common stylesheet.

use shared::types::player::PlayerId;

use super::common::{escape, nav_html, CSS};
use crate::admin::AdminRole;

/// One preview line in a session card. `flagged_word` gets the red highlight
/// so flags stay visible from every view — a moderator inside one session
/// still spots blacklisted words surfacing in the other sessions' previews.
pub struct PreviewLine {
    pub text: String,
    pub flagged_word: Option<String>,
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
    /// The blacklisted word that tripped the flag (highlighted red).
    pub word: String,
}

/// One session's flagged messages, shown as a card on the landing page.
pub struct FlaggedSession {
    pub code: String,
    pub bodies: Vec<FlaggedBody>,
}

/// One transcript row in the session view.
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
    /// Present when the body contains a blacklisted word to highlight.
    pub flagged_word: Option<String>,
    /// True when a moderator spoke into the session rather than a player. Drives
    /// the `MOD` tag so their line is never mistaken for a player's.
    pub is_moderator: bool,
}

/// The Chat Audit Logs are split into category tables (chosen by a dropdown),
/// each with its own direct headers. Every category shares the same leading
/// "who / when / what / why" columns — Timestamp, Display Name, Group, Action,
/// Reason — then adds its own subject/evidence columns. `AuditEntry` groups the
/// three category records so a handler can pass all of them to the page.
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
    /// Whether a chat snapshot exists to open from the Transcript cell.
    pub has_transcript: bool,
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

/// Page-specific styles, appended after the shared `{CSS}`. A plain const
/// (rather than text inside `format!`) so none of the braces need doubling.
const CHATMOD_CSS: &str = "
/* Chat-Mod: full-width three-column moderation layout. The left column width
   is a CSS variable so the drag handle can resize it on desktop. */
.cm-page { width: 100%; }
.cm-card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 24px; }
.cm-layout { display: grid; grid-template-columns: var(--cm-left-w, 280px) minmax(0, 1fr) 260px; gap: 16px; padding-top: 14px; }
.cm-panel { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 14px; }
.cm-panel-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; padding-bottom: 8px; margin-bottom: 10px; border-bottom: 1px solid #30363d; }
.cm-empty { font-size: 0.82rem; color: #8b949e; text-align: center; padding: 14px 0; }
/* left panel: session cards + desktop resize handle (rides in the grid gap).
   Capped flex column — the title stays put, the card list scrolls inside. */
.cm-left { position: relative; }
.cm-left, .cm-right { display: flex; flex-direction: column; max-height: 80vh; }
.cm-panel-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-resize { position: absolute; top: 0; bottom: 0; right: -12px; width: 8px; border-radius: 4px; cursor: col-resize; touch-action: none; background: none; border: none; padding: 0; }
.cm-resize:hover, .cm-resize:focus-visible, body.cm-resizing .cm-resize { background: #30363d; outline: none; }
body.cm-resizing { cursor: col-resize; user-select: none; }
.cm-session-card { position: relative; display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; text-decoration: none; color: #c9d1d9; }
.cm-session-card:hover { border-color: #8b949e; }
.cm-session-card.cm-session-active { border-color: #e94560; }
.cm-session-code { font-size: 0.72rem; color: #8b949e; margin-bottom: 4px; }
.cm-session-preview { font-size: 0.78rem; color: #8b949e; line-height: 1.5; overflow-wrap: anywhere; }
.cm-dot { position: absolute; top: 9px; right: 9px; width: 10px; height: 10px; border-radius: 50%; background: #f85149; }
/* center column */
.cm-center { min-width: 0; }
.cm-subhead { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding-bottom: 10px; margin-bottom: 14px; border-bottom: 1px solid #30363d; }
.cm-subhead-title { flex: 1; font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; }
.cm-burger { display: none; cursor: pointer; font-size: 1.25rem; line-height: 1; color: #c9d1d9; background: none; border: none; padding: 2px 8px; }
/* landing: flagged messages — an inset panel with cards, mirroring the left panel */
.cm-flag-wrap { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 16px 14px; min-height: 62vh; max-height: 78vh; display: flex; flex-direction: column; }
.cm-flag-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-flag-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 4px; }
.cm-flag-sub { font-size: 0.82rem; color: #8b949e; margin-bottom: 14px; }
.cm-flag-card { display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 14px; margin-bottom: 12px; text-decoration: none; color: #c9d1d9; }
.cm-flag-card:hover { border-color: #8b949e; }
.cm-flag-code { font-size: 0.85rem; font-weight: 600; margin-bottom: 6px; }
.cm-flag-user { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; font-size: 0.78rem; color: #c9d1d9; margin-bottom: 3px; }
.cm-pid { font-size: 0.72rem; color: #8b949e; }
.cm-flag-body { font-size: 0.84rem; margin-bottom: 8px; overflow-wrap: anywhere; }
.cm-flag-body:last-child { margin-bottom: 0; }
.cm-bodyid { font-size: 0.72rem; color: #8b949e; }
.cm-flag { background: rgba(248, 81, 73, 0.18); color: #f85149; border-radius: 3px; padding: 0 3px; }
/* transcript flags are tappable toggle buttons; selected = solid red */
.cm-flag-btn { font: inherit; border: none; cursor: pointer; }
.cm-flag-btn:hover { background: rgba(248, 81, 73, 0.32); }
.cm-flag-btn:focus-visible { outline: 1px solid #f85149; outline-offset: 1px; }
.cm-flag-sel { background: #f85149; color: #0d1117; font-weight: 600; }
/* session view */
.cm-session-head { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; }
.cm-session-headtext { flex: 1; min-width: 0; }
.cm-session-title { font-size: 0.95rem; font-weight: 600; margin-bottom: 3px; }
.cm-code { color: #e94560; }
.cm-session-sub { font-size: 0.82rem; color: #8b949e; }
.cm-close { color: #8b949e; text-decoration: none; font-size: 1.05rem; line-height: 1; padding: 4px 8px; border-radius: 6px; background: none; border: none; cursor: pointer; font-family: inherit; }
.cm-close:hover { color: #f85149; background: #2d1117; }
.cm-session-body { display: grid; grid-template-columns: minmax(0, 2fr) minmax(240px, 1fr); gap: 16px; }
.cm-chat { display: flex; flex-direction: column; background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 14px; min-height: 56vh; max-height: 76vh; }
.cm-chat-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cm-msg { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; }
.cm-msg-head { display: flex; align-items: center; gap: 10px; font-size: 0.72rem; color: #8b949e; margin-bottom: 6px; }
.cm-msg-user { flex: 1; color: #c9d1d9; }
.cm-msg-body { font-size: 0.85rem; overflow-wrap: anywhere; }
.cm-chatbar { display: flex; gap: 8px; margin-top: 12px; }
/* quick access tools — grouped: player actions / word tools / settings.
   Capped like the chat column; the groups scroll under the pinned title. */
.cm-tools { display: flex; flex-direction: column; max-height: 76vh; }
.cm-tool-group { margin-top: 12px; }
.cm-tool-group + .cm-tool-group { border-top: 1px solid #30363d; margin-top: 18px; padding-top: 14px; }
.cm-tool-group-title { font-size: 0.66rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.1px; color: #8b949e; margin-bottom: 10px; }
.cm-tool { margin-bottom: 14px; }
.cm-tool:last-child { margin-bottom: 0; }
.cm-tool-btn { width: 100%; }
.cm-check { display: flex; align-items: center; gap: 8px; font-size: 0.84rem; color: #c9d1d9; }
.cm-reason { margin-top: 8px; }
.cm-dur { min-width: 0; }
.cm-approve-all { margin-top: 8px; }
.cm-approve-sel { font-size: 0.74rem; color: #8b949e; margin-top: 6px; }
/* right panel: placeholder secondary nav */
.cm-nav-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 9px 8px; font-size: 0.85rem; color: #8b949e; border-bottom: 1px solid #21262d; cursor: not-allowed; }
.cm-soon { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 1px; color: #6e7681; border: 1px solid #30363d; border-radius: 10px; padding: 1px 7px; }
/* click/tap-to-copy ids: subtle affordance, brief green flash on success */
[data-copy] { cursor: copy; }
[data-copy]:hover { color: #c9d1d9; text-decoration: underline dotted; }
[data-copy]:focus-visible { outline: 1px solid #388bfd; outline-offset: 1px; border-radius: 3px; }
[data-copy].cm-copied, [data-copy].cm-copied:hover { color: #3fb950; }
/* mobile: side panels become edge drawers toggled by the corner burgers */
.cm-backdrop { display: none; position: fixed; inset: 0; background: rgba(1, 4, 9, 0.6); z-index: 90; }
@media (max-width: 768px) {
  .cm-card { padding: 20px 16px; }
  .cm-layout { grid-template-columns: 1fr; }
  .cm-burger { display: inline-block; }
  .cm-resize { display: none; }
  .cm-subhead-title { text-align: center; }
  .cm-left, .cm-right { position: fixed; top: 0; height: 100vh; width: 280px; max-width: 85vw; overflow-y: auto; border-radius: 0; background: #161b22; z-index: 100; visibility: hidden; transition: transform .2s ease, visibility 0s linear .2s; }
  .cm-left { left: 0; border-right: 1px solid #30363d; transform: translateX(-100%); max-height: none; }
  .cm-right { right: 0; border-left: 1px solid #30363d; transform: translateX(100%); max-height: none; }
  body.cm-left-open .cm-left, body.cm-right-open .cm-right { transform: translateX(0); visibility: visible; transition: transform .2s ease; }
  body.cm-left-open .cm-backdrop, body.cm-right-open .cm-backdrop { display: block; }
  .cm-session-body { grid-template-columns: 1fr; }
  .cm-chat { min-height: 46vh; }
  /* stacked layout scrolls as one page — no nested scroll inside the tools */
  .cm-tools { max-height: none; }
  .cm-tools .cm-panel-scroll { flex: none; min-height: auto; overflow-y: visible; }
}
/* right nav: the live Chat Audit Logs entry — a link, or the current-page
   marker — overriding the inert placeholders' cursor:not-allowed */
.cm-nav-link { color: #c9d1d9; text-decoration: none; cursor: pointer; }
.cm-nav-link:hover { color: #fff; background: #161b22; }
.cm-nav-link:focus-visible { outline: 1px solid #388bfd; outline-offset: -1px; }
.cm-nav-current { color: #e94560; cursor: default; }
/* audit logs: advanced-filter placeholder bar + wide scrollable ledger table */
.cm-audit-filter { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 12px 14px; margin-bottom: 14px; }
.cm-audit-filter-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 6px; }
.cm-audit-filter-note { font-size: 0.8rem; color: #6e7681; min-height: 34px; }
/* category dropdown: picks which log table + filter renders */
.cm-audit-select { display: flex; align-items: center; gap: 10px; margin-bottom: 14px; }
.cm-audit-select label { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; }
.cm-audit-select select { min-width: 180px; }
/* per-table Advanced Filter: shared spine group beside the table-specific one */
.cm-filter-grid { display: flex; flex-wrap: wrap; gap: 14px; }
.cm-filter-group { flex: 1; min-width: 260px; border: 1px solid #30363d; border-radius: 8px; padding: 6px 14px 14px; margin: 0; display: flex; flex-wrap: wrap; gap: 10px 14px; }
.cm-filter-group legend { font-size: 0.64rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; padding: 0 6px; }
.cm-filter-group label { display: flex; flex-direction: column; gap: 4px; font-size: 0.7rem; color: #8b949e; }
.cm-filter-group input, .cm-filter-group select { min-width: 130px; }
.cm-audit-list { display: inline-block; background: #21262d; border: 1px solid #30363d; border-radius: 10px; padding: 1px 9px; font-size: 0.74rem; color: #c9d1d9; }
/* System (automated) actor badge — distinct from the red word flags so
   program-initiated rows stand out in any table */
.cm-audit-sys { display: inline-block; background: #10262b; border: 1px solid #1f6f78; border-radius: 10px; padding: 1px 9px; font-size: 0.72rem; font-weight: 600; color: #39c0c8; }
.cm-audit-scroll { border: 1px solid #30363d; border-radius: 8px; overflow: auto; max-height: 68vh; }
.cm-audit-table { width: 100%; border-collapse: collapse; font-size: 0.82rem; white-space: nowrap; }
.cm-audit-table th { position: sticky; top: 0; background: #161b22; text-align: left; font-size: 0.66rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; padding: 10px 12px; border-bottom: 1px solid #30363d; }
/* sortable column header: label + a flip-arrow (triangle with a line under it)
   that rotates 180° to reverse the row order */
.cm-sort { display: inline-flex; align-items: center; gap: 6px; background: none; border: none; padding: 0; cursor: pointer; font: inherit; color: inherit; text-transform: inherit; letter-spacing: inherit; }
.cm-sort:hover, .cm-sort[data-dir] { color: #c9d1d9; }
.cm-sort:focus-visible { outline: 1px solid #388bfd; outline-offset: 2px; }
.cm-sort-ico { display: inline-block; font-size: 0.85em; line-height: 1; border-bottom: 1px solid currentColor; padding-bottom: 1px; transition: transform .15s ease; }
.cm-sort[data-dir='asc'] .cm-sort-ico { transform: rotate(180deg); }
.cm-audit-table td { padding: 10px 12px; border-bottom: 1px solid #21262d; color: #c9d1d9; vertical-align: top; }
.cm-audit-table tbody tr:last-child td { border-bottom: none; }
.cm-audit-table tbody tr:hover td { background: #10151c; }
.cm-audit-ts { color: #8b949e; }
.cm-audit-words { white-space: normal; }
.cm-audit-words .cm-flag { margin-right: 4px; }
.cm-audit-none { color: #6e7681; }
/* multi-body cell: a ×N disclosure condensing one player's covered bodies */
.cm-audit-bodies > summary { cursor: pointer; list-style: none; }
.cm-audit-bodies > summary::-webkit-details-marker { display: none; }
.cm-audit-badge { display: inline-block; background: #21262d; border: 1px solid #30363d; border-radius: 10px; padding: 1px 9px; font-size: 0.72rem; color: #c9d1d9; }
.cm-audit-bodies > summary:hover .cm-audit-badge, .cm-audit-bodies[open] > summary .cm-audit-badge { border-color: #8b949e; }
.cm-audit-bodylist { list-style: none; margin: 8px 0 0; padding: 0; }
.cm-audit-bodylist li { padding: 2px 0; }
/* snapshot: mark the message bodies this action actually covered */
.cm-msg-targeted { border-left: 3px solid #d29922; }
.cm-msg-tag { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #d29922; border: 1px solid #d29922; border-radius: 10px; padding: 0 6px; }
.cm-msg-mod { display: inline-block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 1px; color: #58a6ff; border: 1px solid #58a6ff; border-radius: 10px; padding: 0 6px; margin-right: 4px; }
/* transcript snapshot overlay: wider/taller than the shared 380px modal card so
   the chat reads cleanly; the shared semi-transparent backdrop keeps the page
   visible behind it */
.cm-audit-modal { max-width: 720px; max-height: 82vh; padding: 0; display: flex; flex-direction: column; }
.cm-audit-modal-head { display: flex; align-items: flex-start; gap: 12px; padding: 20px 22px 14px; border-bottom: 1px solid #30363d; }
.cm-audit-modal-head > div { flex: 1; min-width: 0; }
.cm-audit-modal-head .section-sub { margin-bottom: 0; overflow-wrap: anywhere; }
.cm-audit-modal-scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 16px 22px 20px; }
/* Moderation Lists: sub-tab strip + per-tab tools grid. All cm-lists-* so they
   can't collide with the audit/session styles that share this sheet. */
.cm-lists-tabs { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 16px; }
.cm-lists-tab { font: inherit; font-size: 0.82rem; color: #8b949e; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 7px 14px; cursor: pointer; }
.cm-lists-tab:hover { border-color: #8b949e; color: #c9d1d9; }
.cm-lists-tab:focus-visible { outline: 1px solid #388bfd; outline-offset: 1px; }
.cm-lists-tab-active, .cm-lists-tab-active:hover { color: #fff; border-color: #e94560; background: #21262d; cursor: default; }
/* the rounded tools panel above each list table (mirrors the mockup's oval) */
.cm-lists-tools { background: #0d1117; border: 1px solid #30363d; border-radius: 12px; padding: 16px; margin-bottom: 18px; display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 16px 20px; align-items: start; }
.cm-lists-tool { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
/* Lock the reason/text inputs to their own height: with align-items:start the
   columns no longer stretch to a shared height, and flex:0 0 auto keeps a growing
   word box from resizing the reason field above or beside it. */
.cm-lists-tool > input { flex: 0 0 auto; }
.cm-lists-tool-title { font-size: 0.72rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1px; color: #8b949e; }
.cm-lists-tool textarea { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; color: #c9d1d9; padding: 8px 10px; font: inherit; font-size: 0.85rem; resize: vertical; min-height: 88px; overflow: hidden; outline: none; width: 100%; }
.cm-lists-tool textarea:focus { border-color: #388bfd; }
.cm-lists-note { font-size: 0.8rem; color: #8b949e; margin-bottom: 14px; }
.cm-lists-search { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; flex-wrap: wrap; }
.cm-lists-search label { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; white-space: nowrap; }
.cm-lists-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; margin-bottom: 10px; }
.cm-lists-hint { font-size: 0.8rem; color: #8b949e; }
/* checkbox + delete cells in the list tables sit centered */
.cm-lists-check { text-align: center; }
";

/// The center sub-header shared by both views: title bar with a burger at each
/// corner. The burgers are hidden ≥768px; below that they slide the left
/// (sessions) / right (chat nav) panels in as drawers.
const SUBHEAD_HTML: &str = r#"<div class="cm-subhead">
  <button type="button" class="cm-burger" aria-label="Open sessions panel" aria-controls="cm-left" aria-expanded="false" onclick="bbCmToggle(this,'left')">&#9776;</button>
  <span class="cm-subhead-title">Chat Moderation Area</span>
  <button type="button" class="cm-burger" aria-label="Open chat nav panel" aria-controls="cm-right" aria-expanded="false" onclick="bbCmToggle(this,'right')">&#9776;</button>
</div>"#;

/// Which Chat Nav sub-page is currently rendered. Drives the live-link vs
/// current-page-marker choice for the two wired entries (Moderation Lists, Chat
/// Audit Logs); `None` for the landing/session views, where neither is open.
#[derive(Clone, Copy, PartialEq)]
enum ChatNavPage {
    None,
    Lists,
    Audit,
}

/// The right-hand "Chat Nav" secondary navigation. Two entries stay inert visual
/// placeholders until their sub-pages are designed (Settings, Mod User Settings);
/// "Moderation Lists" and "Chat Audit Logs" are live — each a link to its
/// `?from=`-carrying href, or the active current-page marker (no href) when it is
/// the page being rendered. The old standalone "Suspensions" entry now lives as
/// the *Active Suspensions* sub-tab inside Moderation Lists.
fn chat_nav_html(lists_href: &str, audit_href: &str, current: ChatNavPage) -> String {
    // One live entry: the current-page marker when it's the open page, else a
    // link that carries the session context forward.
    let entry = |label: &str, href: &str, is_current: bool| {
        if is_current {
            format!(r#"<span class="cm-nav-item cm-nav-current" aria-current="page">{label}</span>"#)
        } else {
            format!(r#"<a class="cm-nav-item cm-nav-link" href="{href}">{label}</a>"#)
        }
    };
    let lists = entry("Moderation Lists", lists_href, current == ChatNavPage::Lists);
    let audit = entry("Chat Audit Logs", audit_href, current == ChatNavPage::Audit);
    format!(
        r#"<span class="cm-nav-item">Settings<span class="cm-soon">soon</span></span>
        {lists}
        {audit}
        <span class="cm-nav-item">Mod User Settings<span class="cm-soon">soon</span></span>"#
    )
}

/// The session view's "Quick Access Tools" panel. Deliberately styled as the
/// finished controls (not grayed out) so the final look can be judged — but
/// nothing is wired: buttons are `type="button"` with no handlers.
const TOOLS_HTML: &str = r#"<div class="cm-panel cm-tools">
      <p class="cm-panel-title">Quick Access Tools</p>
      <div class="cm-panel-scroll">
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Player Actions</p>
        <div class="cm-tool">
          <label for="cm-target">Target Player IDs</label>
          <input type="text" id="cm-target" placeholder="Player IDs &mdash; separate with ;">
          <p class="cm-approve-sel">Used by Warn, Suspend &amp; Ban. Ticking message checkboxes adds each body's sender.</p>
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn">Warn + Delete Chat Body</button>
          <input type="text" class="cm-reason" placeholder="Reason (logged &amp; sent to player)">
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn">Warn Only</button>
          <input type="text" class="cm-reason" placeholder="Reason (logged &amp; sent to player)">
        </div>
        <div class="cm-tool">
          <label>Suspend User</label>
          <div class="row">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Days" aria-label="Days">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Hours" aria-label="Hours">
            <input type="text" class="cm-dur" inputmode="numeric" placeholder="Mins" aria-label="Minutes">
            <button type="button" class="btn btn-sm">Suspend</button>
          </div>
          <input type="text" class="cm-reason" placeholder="Reason (logged &amp; sent to player)">
          <p class="cm-approve-sel">At least one duration field is required.</p>
        </div>
        <div class="cm-tool">
          <label>Ban User (Chat)</label>
          <div class="row">
            <input type="text" id="cm-ban-reason" placeholder="Reason (logged &amp; sent to player)">
            <button type="button" class="btn btn-danger btn-sm" onclick="bbCmBanAsk()">Ban</button>
          </div>
        </div>
      </div>
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Word Tools</p>
        <div class="cm-tool">
          <label for="cm-blacklist">Blacklist Words</label>
          <div class="row">
            <input type="text" id="cm-blacklist" placeholder="Word or words &mdash; separate with ;">
            <button type="button" class="btn btn-sm">Add</button>
          </div>
          <input type="text" class="cm-reason" placeholder="Reason (logged)">
        </div>
        <div class="cm-tool">
          <button type="button" class="btn btn-sm cm-tool-btn">Approve Word</button>
          <p class="cm-approve-sel">Restores the word in this chat &mdash; blacklist unchanged.</p>
          <label class="cm-check cm-approve-all"><input type="checkbox" id="cm-approve-all"> Select all matching words</label>
          <p class="cm-approve-sel" id="cm-approve-sel">Tap a red word in the chat to select it.</p>
        </div>
      </div>
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Moderator Chat Settings</p>
        <label class="cm-check"><input type="checkbox"> Appear As Your Display Name</label>
      </div>
      <p class="note">Preview only &mdash; tools are not wired up yet.</p>
      </div>
    </div>
    <div id="cm-ban-modal" class="modal-backdrop" onclick="if(event.target===this)bbCmBanClose()">
      <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-ban-title" aria-describedby="cm-ban-desc">
        <p class="section-title" id="cm-ban-title">Confirm Chat Ban</p>
        <p class="section-sub" id="cm-ban-desc">Permanently remove chat privileges for <span id="cm-ban-who" class="mono"></span>?<br>Reason: <span id="cm-ban-why"></span><br>The player keeps playing; reversible only via Moderation Lists.</p>
        <div class="modal-actions">
          <button type="button" class="btn btn-sm" id="cm-ban-cancel" onclick="bbCmBanClose()">Cancel</button>
          <button type="button" class="btn btn-danger btn-sm" onclick="bbCmBanClose()">Confirm Chat Ban</button>
        </div>
      </div>
    </div>"#;

/// Render `body` with every standalone occurrence of `word` replaced by the
/// prebuilt `wrapped` markup. Matching runs on the raw (unescaped) text with
/// word-boundary checks — an adjacent alphanumeric disqualifies a match, so
/// "scrub" never fires inside "scrubbing" — and each non-match segment is
/// escaped separately, so a match can never land inside an HTML entity
/// produced by escaping. Case-sensitive; the wiring phase's flagging engine
/// owns smarter matching and will hand over exact occurrences.
fn highlight_with(body: &str, word: &str, wrapped: &str) -> String {
    if word.is_empty() {
        return escape(body);
    }
    let mut out = String::new();
    let mut rest = body;
    while let Some(idx) = rest.find(word) {
        let end = idx + word.len();
        let boundary_before = rest[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let boundary_after = rest[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if boundary_before && boundary_after {
            out.push_str(&escape(&rest[..idx]));
            out.push_str(wrapped);
        } else {
            // Substring of a larger word — pass it through untouched.
            out.push_str(&escape(&rest[..end]));
        }
        rest = &rest[end..];
    }
    out.push_str(&escape(rest));
    out
}

/// Wrap standalone occurrences of the blacklisted `word` in the red
/// highlight span (see [`highlight_with`] for the matching rules).
fn highlight(body: &str, word: &str) -> String {
    let needle = escape(word);
    highlight_with(
        body,
        word,
        &format!(r#"<span class="cm-flag">{needle}</span>"#),
    )
}

/// Transcript variant of [`highlight`]: each occurrence renders as an inline
/// toggle button so moderators can tap words to build the Approve selection.
/// Per-instance by default; the "Select all matching words" checkbox in the
/// tools panel widens a tap to every occurrence of that word (`data-word`).
fn highlight_toggle(body: &str, word: &str) -> String {
    let needle = escape(word);
    highlight_with(
        body,
        word,
        &format!(
            r#"<button type="button" class="cm-flag cm-flag-btn" data-word="{needle}" aria-pressed="false" onclick="bbCmFlagToggle(this)">{needle}</button>"#
        ),
    )
}

/// The left "Active Game Sessions" card list. `active_code` highlights the
/// session currently entered (session view only).
fn session_list_html(sessions: &[ChatSession], active_code: Option<&str>) -> String {
    if sessions.is_empty() {
        return r#"<p class="cm-empty">No active sessions.</p>"#.to_string();
    }
    sessions
        .iter()
        .map(|s| {
            let code = escape(&s.code);
            let active = if active_code == Some(s.code.as_str()) {
                " cm-session-active"
            } else {
                ""
            };
            let dot = if s.flagged {
                r#"<span class="cm-dot" title="Has flagged messages"></span>"#
            } else {
                ""
            };
            let preview = s
                .preview
                .iter()
                .map(|line| match &line.flagged_word {
                    Some(word) => highlight(&line.text, word),
                    None => escape(&line.text),
                })
                .collect::<Vec<_>>()
                .join("<br>");
            format!(
                r#"<a href="/admin/chatmod/session/{code}" class="cm-session-card{active}">{dot}
          <div class="cm-session-code">code: {code}</div>
          <div class="cm-session-preview">{preview}</div>
        </a>"#
            )
        })
        .collect()
}

/// The landing page's flagged-message cards — one per session with flags, each
/// linking into that session's view.
fn flagged_list_html(flagged: &[FlaggedSession]) -> String {
    if flagged.is_empty() {
        return r#"<p class="cm-empty">No flagged messages.</p>"#.to_string();
    }
    flagged
        .iter()
        .map(|f| {
            let code = escape(&f.code);
            let bodies = f
                .bodies
                .iter()
                .map(|b| {
                    format!(
                        r#"<div class="cm-flag-user"><span>{user} <span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span></span><span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></div>
          <div class="cm-flag-body">{body}</div>"#,
                        user = escape(&b.username),
                        pid = PlayerId::from_counter(b.player_id),
                        body = highlight(&b.body, &b.word),
                        id = escape(&b.body_id),
                    )
                })
                .collect::<String>();
            format!(
                r#"<a href="/admin/chatmod/session/{code}" class="cm-flag-card">
          <div class="cm-flag-code">Code: {code}</div>
          {bodies}
        </a>"#
            )
        })
        .collect()
}

/// The session view's transcript rows: body identifier + username header (with
/// a select checkbox for the tools panel) above the message text.
fn transcript_html(transcript: &[ChatMessage]) -> String {
    if transcript.is_empty() {
        return r#"<p class="cm-empty">No messages in this session yet.</p>"#.to_string();
    }
    transcript
        .iter()
        .map(|m| {
            let body = match &m.flagged_word {
                Some(word) => highlight_toggle(&m.body, word),
                None => escape(&m.body),
            };
            let id = escape(&m.body_id);
            // A moderator line carries no player id and no select checkbox —
            // the player tools act on player accounts, and a moderator is not
            // one. The MOD tag keeps it from reading as a player's message.
            let (pid_chip, select) = match m.player_id {
                Some(pid) => {
                    let pid = PlayerId::from_counter(pid);
                    (
                        format!(
                            r#"<span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span> "#
                        ),
                        format!(r#"<input type="checkbox" data-pid="{pid}" aria-label="Select message {id}">"#),
                    )
                }
                None => (String::new(), String::new()),
            };
            let mod_tag = if m.is_moderator {
                r#"<span class="cm-msg-mod">MOD</span> "#
            } else {
                ""
            };
            format!(
                r#"<div class="cm-msg">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{mod_tag}{user} {pid_chip}<span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></span>
              {select}
            </div>
            <div class="cm-msg-body">{body}</div>
          </div>"#,
                user = escape(&m.username),
            )
        })
        .collect()
}

/// The full-width three-column document both views share: nav, left sessions
/// panel, injected `center`, right Chat Nav panel, and the drawer-toggle
/// script. Braces in the inline JS are doubled for `format!`.
fn chatmod_shell(
    center: &str,
    sessions: &[ChatSession],
    active_code: Option<&str>,
    nav_from: Option<&str>,
    current: ChatNavPage,
    role: AdminRole,
    username: &str,
) -> String {
    let nav = nav_html("chatmod", role, username);
    let session_cards = session_list_html(sessions, active_code);
    // The Chat Nav "Moderation Lists" / "Chat Audit Logs" links carry the session
    // the moderator came from (via ?from=). This is `nav_from`, kept separate from
    // `active_code` (which only highlights the entered session's card): a sub-page
    // isn't "in" a session, yet must still forward the context so hopping between
    // the two sub-pages — and each one's X — returns to that session, not the
    // landing page.
    let (lists_href, audit_href) = match nav_from {
        Some(code) => {
            let c = escape(code);
            (
                format!("/admin/chatmod/lists?from={c}"),
                format!("/admin/chatmod/audit?from={c}"),
            )
        }
        None => (
            "/admin/chatmod/lists".to_string(),
            "/admin/chatmod/audit".to_string(),
        ),
    };
    let chat_nav = chat_nav_html(&lists_href, &audit_href, current);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin Chat-Mod</title>
  <style>{CSS}{CHATMOD_CSS}</style>
</head>
<body>
<div class="cm-page">
  <div class="cm-card">
    {nav}
    <div class="cm-layout">
      <aside class="cm-panel cm-left" id="cm-left">
        <p class="cm-panel-title">Active Game Sessions</p>
        <div class="cm-panel-scroll">
        {session_cards}
        </div>
        <button type="button" class="cm-resize" id="cm-resize" aria-label="Resize sessions panel (drag, arrow keys, double-click resets)"></button>
      </aside>
      <main class="cm-center">
        {center}
      </main>
      <aside class="cm-panel cm-right" id="cm-right">
        <p class="cm-panel-title">Chat Nav</p>
        <div class="cm-panel-scroll">
        {chat_nav}
        </div>
      </aside>
    </div>
    <div class="cm-backdrop" onclick="bbCmClose()"></div>
  </div>
</div>
<script>
function bbCmToggle(btn,side){{var cls=side==='left'?'cm-left-open':'cm-right-open';document.body.classList.remove(side==='left'?'cm-right-open':'cm-left-open');var open=document.body.classList.toggle(cls);btn.setAttribute('aria-expanded',open?'true':'false');}}
function bbCmClose(){{document.body.classList.remove('cm-left-open','cm-right-open');document.querySelectorAll('.cm-burger').forEach(function(b){{b.setAttribute('aria-expanded','false');}});}}
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCmClose();}});
// Chat opens scrolled to the newest message, chat-app style; the flex layout
// keeps the moderator chat bar pinned to the container's bottom edge.
(function(){{var sc=document.querySelector('.cm-chat-scroll');if(sc)sc.scrollTop=sc.scrollHeight;}})();
// Word selection for Approve: each red word in the transcript toggles
// individually; the "Select all matching words" checkbox widens a tap to
// every occurrence of that word. Selection state lives on the buttons
// (cm-flag-sel class), the hint line aggregates it per word.
window.bbCmFlagToggle=function(btn){{
  var on=!btn.classList.contains('cm-flag-sel');
  var all=document.getElementById('cm-approve-all');
  var targets=(all&&all.checked)?document.querySelectorAll('.cm-flag-btn[data-word="'+CSS.escape(btn.getAttribute('data-word'))+'"]'):[btn];
  targets.forEach(function(b){{b.classList.toggle('cm-flag-sel',on);b.setAttribute('aria-pressed',on?'true':'false');}});
  var el=document.getElementById('cm-approve-sel');
  if(!el)return;
  var counts={{}};
  document.querySelectorAll('.cm-flag-btn.cm-flag-sel').forEach(function(b){{var w=b.getAttribute('data-word');counts[w]=(counts[w]||0)+1;}});
  var parts=Object.keys(counts).map(function(w){{return counts[w]>1?w+' ×'+counts[w]:w;}});
  el.textContent=parts.length?('Selected: '+parts.join(', ')):'Tap a red word in the chat to select it.';
}};
// Click/tap-to-copy for player ids + body ids: one delegated listener over
// [data-copy] elements. preventDefault stops the landing cards' link
// navigation when the tap lands on an id. Clipboard API first (secure
// contexts), hidden-textarea execCommand fallback otherwise.
function bbCmDoCopy(el){{
  var v=el.getAttribute('data-copy');
  function done(){{el.classList.add('cm-copied');setTimeout(function(){{el.classList.remove('cm-copied');}},900);}}
  function fallback(){{
    var t=document.createElement('textarea');t.value=v;t.style.position='fixed';t.style.opacity='0';
    document.body.appendChild(t);t.select();
    try{{document.execCommand('copy');}}catch(e){{}}
    document.body.removeChild(t);done();
  }}
  if(navigator.clipboard&&navigator.clipboard.writeText){{navigator.clipboard.writeText(v).then(done).catch(fallback);}}
  else{{fallback();}}
}}
document.addEventListener('click',function(e){{
  var el=e.target.closest('[data-copy]');
  if(!el)return;
  e.preventDefault();
  bbCmDoCopy(el);
}});
document.addEventListener('keydown',function(e){{
  if(e.key!=='Enter'&&e.key!==' ')return;
  var el=e.target.closest&&e.target.closest('[data-copy]');
  if(!el)return;
  e.preventDefault();
  bbCmDoCopy(el);
}});
// Message checkboxes feed the Target Player IDs field, a ;-separated list
// (same separator convention as Blacklist Words). Ticking a body appends its
// sender once; unticking removes the id only when no other ticked body still
// carries it. Manually typed ids survive — the list is edited per-id, never
// overwritten wholesale. Only elements carrying data-pid reach this handler.
document.addEventListener('change',function(e){{
  var cb=e.target;
  if(!cb.getAttribute||cb.getAttribute('data-pid')===null)return;
  var t=document.getElementById('cm-target');
  if(!t)return;
  var pid=cb.getAttribute('data-pid');
  var list=t.value.split(';').map(function(s){{return s.trim();}}).filter(function(s){{return s;}});
  if(cb.checked){{
    if(list.indexOf(pid)===-1)list.push(pid);
  }}else if(!document.querySelector('input[data-pid="'+CSS.escape(pid)+'"]:checked')){{
    list=list.filter(function(s){{return s!==pid;}});
  }}
  t.value=list.join('; ');
}});
// Ban requires an explicit confirmation: the modal echoes the target ids and
// reason for review. Cancel / backdrop / Escape back out — a cancelled ban is
// never sent and writes no audit record. Confirm just closes for now; the
// wiring phase swaps it for the real send.
window.bbCmBanAsk=function(){{
  var m=document.getElementById('cm-ban-modal');if(!m)return;
  var t=document.getElementById('cm-target'),r=document.getElementById('cm-ban-reason');
  document.getElementById('cm-ban-who').textContent=(t&&t.value.trim())?t.value.trim():'(no target set)';
  document.getElementById('cm-ban-why').textContent=(r&&r.value.trim())?r.value.trim():'(none)';
  m.style.display='flex';
  var c=document.getElementById('cm-ban-cancel');if(c)c.focus();
}};
window.bbCmBanClose=function(){{var m=document.getElementById('cm-ban-modal');if(m)m.style.display='none';}};
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCmBanClose();}});
// Audit-log transcript snapshots: each Chat Audit Logs row's Transcript button
// opens that row's own hidden overlay (index-aligned with the table). Close via
// the overlay X, Escape, or a backdrop click. Preview only — the wiring phase
// fetches the real snapshot on demand instead of pre-rendering one per row.
window.bbCmAuditOpen=function(i){{var m=document.getElementById('cm-audit-back-'+i);if(m)m.style.display='flex';}};
window.bbCmAuditCloseAll=function(){{document.querySelectorAll('.cm-audit-back').forEach(function(m){{m.style.display='none';}});}};
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCmAuditCloseAll();}});
// Chat Audit Logs category dropdown: show the picked category's view, hide the
// rest. Each view carries its own table + Advanced Filter, so switching swaps
// both at once.
window.bbCmAuditCat=function(sel){{document.querySelectorAll('.cm-audit-view').forEach(function(v){{v.hidden=(v.getAttribute('data-cat')!==sel.value);}});}};
// Sortable audit columns (Timestamp, Player ID): reorder the table's rows by the
// clicked column and flip the arrow. First click sorts ascending (reversing the
// default newest-first / server order); clicking again toggles. Only one column
// sorts a table at a time. Timestamps (ISO-ish) and zero-padded ids compare
// correctly with a numeric-aware string compare.
window.bbCmSort=function(btn){{
  var th=btn.closest('th'), table=btn.closest('table');
  if(!th||!table||!table.tBodies[0])return;
  var col=th.cellIndex, tbody=table.tBodies[0];
  var dir=btn.getAttribute('data-dir')==='asc'?'desc':'asc';
  table.querySelectorAll('.cm-sort').forEach(function(b){{if(b!==btn){{b.removeAttribute('data-dir');var ot=b.closest('th');if(ot)ot.setAttribute('aria-sort','none');}}}});
  btn.setAttribute('data-dir',dir);
  th.setAttribute('aria-sort',dir==='asc'?'ascending':'descending');
  var rows=Array.prototype.slice.call(tbody.rows);
  rows.sort(function(a,b){{
    var x=(a.cells[col]?a.cells[col].textContent:'').trim();
    var y=(b.cells[col]?b.cells[col].textContent:'').trim();
    var r=x.localeCompare(y,undefined,{{numeric:true}});
    return dir==='asc'?r:-r;
  }});
  rows.forEach(function(r){{tbody.appendChild(r);}});
}};
// Desktop-only resize of the sessions panel: drag writes the grid's CSS
// variable, localStorage persists it across the landing/session page loads.
(function(){{
  var KEY='bb_cm_left_w', MIN=200, MAX=520, root=document.documentElement;
  var saved=parseInt(localStorage.getItem(KEY)||'',10);
  if(saved)root.style.setProperty('--cm-left-w',Math.min(MAX,Math.max(MIN,saved))+'px');
  var h=document.getElementById('cm-resize'), panel=document.getElementById('cm-left');
  if(!h||!panel)return;
  function apply(w){{w=Math.min(MAX,Math.max(MIN,Math.round(w)));root.style.setProperty('--cm-left-w',w+'px');try{{localStorage.setItem(KEY,String(w));}}catch(e){{}}}}
  h.addEventListener('pointerdown',function(e){{
    e.preventDefault();
    var startX=e.clientX, startW=panel.getBoundingClientRect().width;
    h.setPointerCapture(e.pointerId);
    document.body.classList.add('cm-resizing');
    function move(ev){{apply(startW+(ev.clientX-startX));}}
    function up(ev){{h.releasePointerCapture(ev.pointerId);h.removeEventListener('pointermove',move);h.removeEventListener('pointerup',up);h.removeEventListener('pointercancel',up);document.body.classList.remove('cm-resizing');}}
    h.addEventListener('pointermove',move);
    h.addEventListener('pointerup',up);
    h.addEventListener('pointercancel',up);
  }});
  h.addEventListener('dblclick',function(){{root.style.removeProperty('--cm-left-w');try{{localStorage.removeItem(KEY);}}catch(e){{}}}});
  h.addEventListener('keydown',function(e){{
    if(e.key!=='ArrowLeft'&&e.key!=='ArrowRight')return;
    e.preventDefault();
    apply(panel.getBoundingClientRect().width+(e.key==='ArrowRight'?16:-16));
  }});
}})();
// Moderation Lists sub-tabs: a horizontal tab strip toggling the four list
// panels. Client-side only — the same show/hide idea as the audit dropdown.
window.bbCmListsTab=function(btn){{
  var tab=btn.getAttribute('data-tab');
  document.querySelectorAll('.cm-lists-tab').forEach(function(b){{
    var on=b.getAttribute('data-tab')===tab;
    b.classList.toggle('cm-lists-tab-active',on);
    b.setAttribute('aria-selected',on?'true':'false');
  }});
  document.querySelectorAll('.cm-lists-panel').forEach(function(p){{
    p.hidden=p.getAttribute('data-tab')!==tab;
  }});
}};
// Blacklist word boxes grow to fit their contents (the mockup's "flex larger in
// height when many words are added"); manual resize still works via CSS.
(function(){{
  function grow(t){{t.style.height='auto';t.style.height=t.scrollHeight+'px';}}
  document.querySelectorAll('.cm-lists-tool textarea').forEach(function(t){{
    grow(t);t.addEventListener('input',function(){{grow(t);}});
  }});
}})();
// Moderation Lists confirm dialogs (delete word / ban / un-ban): open by id,
// close on Cancel / backdrop / Escape. Preview only — Confirm just closes; the
// wiring phase swaps it for the real action.
window.bbCmListsAsk=function(id){{var m=document.getElementById(id);if(m)m.style.display='flex';}};
window.bbCmListsCloseAll=function(){{document.querySelectorAll('.cm-lists-modal').forEach(function(m){{m.style.display='none';}});}};
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCmListsCloseAll();}});
</script>
</body>
</html>"#
    )
}

/// GET /admin/chatmod — the Chat-Mod landing view: flagged-message overview in
/// the center, wrapped in the shared three-column shell.
pub fn chatmod_landing_page(
    sessions: &[ChatSession],
    flagged: &[FlaggedSession],
    role: AdminRole,
    username: &str,
) -> String {
    let flag_cards = flagged_list_html(flagged);
    let center = format!(
        r#"{SUBHEAD_HTML}
<div class="cm-flag-wrap">
  <p class="cm-flag-title">Flagged Messages</p>
  <p class="cm-flag-sub">Messages flagged by the system for containing blacklisted words. Click one to enter that session.</p>
  <div class="cm-flag-scroll">
  {flag_cards}
  </div>
</div>"#
    );
    chatmod_shell(&center, sessions, None, None, ChatNavPage::None, role, username)
}

/// GET /admin/chatmod/session/:code — the entered-session view: transcript +
/// moderator chat bar on the left, Quick Access Tools on the right.
pub fn chatmod_session_page(
    code: &str,
    transcript: &[ChatMessage],
    sessions: &[ChatSession],
    role: AdminRole,
    username: &str,
) -> String {
    let code_html = escape(code);
    let messages = transcript_html(transcript);
    let center = format!(
        r#"{SUBHEAD_HTML}
<div class="cm-session-head">
  <div class="cm-session-headtext">
    <p class="cm-session-title">Session Chat Code: <span class="mono cm-code">{code_html}</span>, You Have Entered</p>
    <p class="cm-session-sub">You can monitor, join the chat when necessary, and use mod tools at your discretion.</p>
  </div>
  <a href="/admin/chatmod" class="cm-close" aria-label="Leave session">&#10005;</a>
</div>
<div class="cm-session-body">
  <div class="cm-chat">
    <div class="cm-chat-scroll">
      {messages}
    </div>
    <div class="cm-chatbar">
      <input type="text" placeholder="Moderator chat bar">
      <button type="button" class="btn btn-primary btn-sm">Send</button>
    </div>
  </div>
  {TOOLS_HTML}
</div>"#
    );
    chatmod_shell(&center, sessions, Some(code), Some(code), ChatNavPage::None, role, username)
}

/// The Body Id table cell: the single id (tap-to-copy), an em-dash for a
/// body-less action, or a `×N bodies` disclosure listing each id (all
/// tap-to-copy) when one entry covered several of that player's messages.
fn audit_bodies_cell(body_ids: &[String]) -> String {
    match body_ids {
        [] => r#"<span class="cm-audit-none">&mdash;</span>"#.to_string(),
        [one] => format!(
            r#"<span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">{id}</span>"#,
            id = escape(one)
        ),
        many => {
            let items = many
                .iter()
                .map(|id| {
                    format!(
                        r#"<li><span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">{id}</span></li>"#,
                        id = escape(id)
                    )
                })
                .collect::<String>();
            format!(
                r#"<details class="cm-audit-bodies"><summary><span class="cm-audit-badge">&times;{n} bodies</span></summary><ul class="cm-audit-bodylist">{items}</ul></details>"#,
                n = many.len()
            )
        }
    }
}

/// Inline body-id descriptor for the snapshot overlay header: the single id, a
/// `×N bodies` count with the list, or a note when the action was body-less.
fn audit_bodies_inline(body_ids: &[String]) -> String {
    match body_ids {
        [] => r#"<span class="cm-audit-none">No specific body</span>"#.to_string(),
        [one] => format!(r#"Body ID: <span class="cm-bodyid mono">{}</span>"#, escape(one)),
        many => {
            let ids = many
                .iter()
                .map(|id| format!(r#"<span class="cm-bodyid mono">{}</span>"#, escape(id)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(r#"&times;{n} bodies: {ids}"#, n = many.len())
        }
    }
}

/// A `Player UserName` + `Player ID` cell pair (the id tap-to-copy), shared by
/// the tables that target a player. `id` absent ⇒ em-dash (e.g. a global
/// Blacklist Word action with no specific sender).
fn audit_player_cells(username: Option<&str>, id: Option<u64>) -> String {
    let name = match username {
        Some(u) => escape(u),
        None => r#"<span class="cm-audit-none">&mdash;</span>"#.to_string(),
    };
    let id_cell = match id {
        Some(id) => {
            let pid = PlayerId::from_counter(id);
            format!(
                r#"<span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">{pid}</span>"#
            )
        }
        None => r#"<span class="cm-audit-none">&mdash;</span>"#.to_string(),
    };
    format!("<td>{name}</td>\n            <td>{id_cell}</td>")
}

/// A Transcript cell: a button opening overlay `dom_id` when there's a snapshot,
/// or a plain dash for actions with no chat context (List rows, Blacklist Word).
fn audit_transcript_cell(dom_id: &str, has_snapshot: bool) -> String {
    if has_snapshot {
        format!(
            r#"<button type="button" class="btn btn-sm" onclick="bbCmAuditOpen('{dom_id}')">Transcript</button>"#
        )
    } else {
        r#"<span class="cm-audit-none">&mdash;</span>"#.to_string()
    }
}

/// The Group cell — a distinct badge when the actor is the program
/// (`Group = System`), so automated rows stand out in any table; plain text for
/// the human roles (Admin / Moderator / Superadmin).
fn audit_group_cell(group: &str) -> String {
    if group == "System" {
        r#"<span class="cm-audit-sys">System</span>"#.to_string()
    } else {
        escape(group)
    }
}

/// **Player** category table (today's headers, unchanged): one row per
/// (action, target player); a player's covered bodies condense into a
/// `×N bodies` disclosure, flagged words render as red chips. A row's actor may
/// be the program (`Group = System`, an auto-enforcement rule) — rendered
/// identically but with the System badge.
fn player_audit_table_html(entries: &[PlayerAuditEntry]) -> String {
    if entries.is_empty() {
        return r#"<p class="cm-empty">No player actions recorded yet.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let words = if e.flagged_words.is_empty() {
                r#"<span class="cm-audit-none">&mdash;</span>"#.to_string()
            } else {
                e.flagged_words
                    .iter()
                    .map(|w| format!(r#"<span class="cm-flag">{}</span>"#, escape(w)))
                    .collect::<String>()
            };
            format!(
                r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{mod_name}</td>
            <td>{group}</td>
            <td>{action}</td>
            <td>{reason}</td>
            {player}
            <td>{bodies}</td>
            <td class="cm-audit-words">{words}</td>
            <td>{transcript}</td>
          </tr>"#,
                ts = escape(&e.timestamp),
                mod_name = escape(&e.moderator_display),
                group = audit_group_cell(&e.moderator_group),
                action = escape(&e.action),
                reason = escape(&e.reason),
                player = audit_player_cells(Some(&e.target_username), Some(e.target_player_id)),
                bodies = audit_bodies_cell(&e.body_ids),
                transcript = audit_transcript_cell(&format!("player-{i}"), !e.snapshot.is_empty()),
            )
        })
        .collect::<String>();
    format!(
        r#"<table class="cm-audit-table">
        <thead>
          <tr>
            <th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Display Name</th><th>Group</th><th>Action</th><th>Reason</th><th>Player UserName</th><th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Player ID<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Body Id</th><th>Flagged Words</th><th>Transcript</th>
          </tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
    )
}

/// **Word** category table: blacklist/approve actions keyed by the word, with
/// the occurrence's sender + body when the action was an Approve.
fn word_audit_table_html(entries: &[WordAuditEntry]) -> String {
    if entries.is_empty() {
        return r#"<p class="cm-empty">No word actions recorded yet.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            format!(
                r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{mod_name}</td>
            <td>{group}</td>
            <td>{action}</td>
            <td>{reason}</td>
            <td><span class="cm-flag">{word}</span></td>
            {player}
            <td>{bodies}</td>
            <td>{transcript}</td>
          </tr>"#,
                ts = escape(&e.timestamp),
                mod_name = escape(&e.moderator_display),
                group = audit_group_cell(&e.moderator_group),
                action = escape(&e.action),
                reason = escape(&e.reason),
                word = escape(&e.word),
                player = audit_player_cells(e.target_username.as_deref(), e.target_player_id),
                bodies = audit_bodies_cell(&e.body_ids),
                transcript = audit_transcript_cell(&format!("word-{i}"), !e.snapshot.is_empty()),
            )
        })
        .collect::<String>();
    format!(
        r#"<table class="cm-audit-table">
        <thead>
          <tr>
            <th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Display Name</th><th>Group</th><th>Action</th><th>Reason</th><th>Word</th><th>Player UserName</th><th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Player ID<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Body Id</th><th>Transcript</th>
          </tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
    )
}

/// **List** category table: moderation-list edits, targeting a player and
/// naming which list. No chat snapshot.
fn list_audit_table_html(entries: &[ListAuditEntry]) -> String {
    if entries.is_empty() {
        return r#"<p class="cm-empty">No list actions recorded yet.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .map(|e| {
            format!(
                r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{mod_name}</td>
            <td>{group}</td>
            <td>{action}</td>
            <td>{reason}</td>
            {player}
            <td><span class="cm-audit-list">{list}</span></td>
          </tr>"#,
                ts = escape(&e.timestamp),
                mod_name = escape(&e.moderator_display),
                group = audit_group_cell(&e.moderator_group),
                action = escape(&e.action),
                reason = escape(&e.reason),
                player = audit_player_cells(Some(&e.target_username), Some(e.target_player_id)),
                list = escape(&e.list),
            )
        })
        .collect::<String>();
    format!(
        r#"<table class="cm-audit-table">
        <thead>
          <tr>
            <th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Display Name</th><th>Group</th><th>Action</th><th>Reason</th><th>Player UserName</th><th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Player ID<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>List</th>
          </tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
    )
}

/// **System** category table: automated non-enforcement events (word flagging).
/// Same columns as the Word table, but every row is `Group = System`, the
/// Display Name is the automated `source`, and Reason holds the `trigger`.
fn system_audit_table_html(entries: &[SystemAuditEntry]) -> String {
    if entries.is_empty() {
        return r#"<p class="cm-empty">No automated actions recorded yet.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            format!(
                r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{source}</td>
            <td>{group}</td>
            <td>{action}</td>
            <td>{trigger}</td>
            <td><span class="cm-flag">{word}</span></td>
            {player}
            <td>{bodies}</td>
            <td>{transcript}</td>
          </tr>"#,
                ts = escape(&e.timestamp),
                source = escape(&e.source),
                group = audit_group_cell("System"),
                action = escape(&e.action),
                trigger = escape(&e.trigger),
                word = escape(&e.word),
                player = audit_player_cells(Some(&e.target_username), Some(e.target_player_id)),
                bodies = audit_bodies_cell(&e.body_ids),
                transcript = audit_transcript_cell(&format!("system-{i}"), !e.snapshot.is_empty()),
            )
        })
        .collect::<String>();
    format!(
        r#"<table class="cm-audit-table">
        <thead>
          <tr>
            <th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Display Name</th><th>Group</th><th>Action</th><th>Reason</th><th>Word</th><th>Player UserName</th><th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Player ID<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button></th><th>Body Id</th><th>Transcript</th>
          </tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
    )
}

/// Snapshot overlays for the System table, keyed `system-{i}`.
fn system_audit_modals_html(entries: &[SystemAuditEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.snapshot.is_empty())
        .map(|(i, e)| {
            let subject = format!(
                r#"Word: <span class="cm-flag">{word}</span> &middot; {u} <span class="cm-pid mono">ID {pid}</span>"#,
                word = escape(&e.word),
                u = escape(&e.target_username),
                pid = PlayerId::from_counter(e.target_player_id),
            );
            audit_modal_html(&format!("system-{i}"), &e.action, &subject, &e.body_ids, &e.snapshot)
        })
        .collect()
}

/// The per-table Advanced Filter panel: a persistent group (the shared spine —
/// carries across tables) beside a table-specific group. `actions` populates the
/// persistent Action dropdown with that table's actions; `specific` is the
/// table-specific fieldset body. Inert placeholders — not wired.
///
/// The From/To time pickers carry `lang="en-GB"` so the native control renders a
/// 24-hour clock (matching the `… UTC` timestamps) despite the page's `en`
/// (12-hour) locale. The eventual per-moderator setting owns 12/24h + timezone.
fn audit_filter_panel(actions: &[&str], specific: &str) -> String {
    let opts = actions
        .iter()
        .map(|a| format!("<option>{}</option>", escape(a)))
        .collect::<String>();
    format!(
        r#"<div class="cm-audit-filter">
      <p class="cm-audit-filter-title">Advanced Filter</p>
      <div class="cm-filter-grid">
        <fieldset class="cm-filter-group">
          <legend>Applies to all logs</legend>
          <label>From date<input type="date"></label>
          <label>To date<input type="date"></label>
          <label>From time (UTC)<input type="time" lang="en-GB"></label>
          <label>To time (UTC)<input type="time" lang="en-GB"></label>
          <label>Moderator<input type="text" placeholder="Display name"></label>
          <label>Group<select><option>Any</option><option>Admin</option><option>Moderator</option><option>Superadmin</option><option>System</option></select></label>
          <label>Action<select><option>Any</option>{opts}</select></label>
          <label>Reason<input type="text" placeholder="Search reason"></label>
        </fieldset>
        <fieldset class="cm-filter-group">
          <legend>This table</legend>
          {specific}
        </fieldset>
      </div>
      <p class="note">Preview only &mdash; filters are not wired up yet.</p>
    </div>"#
    )
}

/// Read-only snapshot rows for the audit overlay: the live transcript's
/// message-card look without the select checkboxes or tappable flag toggles —
/// a frozen record, so blacklisted words use the static red span. Bodies in
/// `targeted` (the entry's `body_ids`) are flagged as "acted on" so the
/// moderator sees exactly which messages the action covered.
fn audit_snapshot_html(snapshot: &[ChatMessage], targeted: &[String]) -> String {
    if snapshot.is_empty() {
        return r#"<p class="cm-empty">No chat captured for this action.</p>"#.to_string();
    }
    snapshot
        .iter()
        .map(|m| {
            let body = match &m.flagged_word {
                Some(word) => highlight(&m.body, word),
                None => escape(&m.body),
            };
            let acted = targeted.iter().any(|id| id == &m.body_id);
            let cls = if acted { " cm-msg-targeted" } else { "" };
            let tag = if acted {
                r#" <span class="cm-msg-tag">acted on</span>"#
            } else {
                ""
            };
            let pid_chip = match m.player_id {
                Some(pid) => format!(
                    r#"<span class="cm-pid mono">ID {}</span> "#,
                    PlayerId::from_counter(pid)
                ),
                None => String::new(),
            };
            let mod_tag = if m.is_moderator {
                r#"<span class="cm-msg-mod">MOD</span> "#
            } else {
                ""
            };
            format!(
                r#"<div class="cm-msg{cls}">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{mod_tag}{user} {pid_chip}<span class="cm-bodyid mono">Body ID: {id}</span>{tag}</span>
            </div>
            <div class="cm-msg-body">{body}</div>
          </div>"#,
                id = escape(&m.body_id),
                user = escape(&m.username),
            )
        })
        .collect()
}

/// One hidden snapshot overlay. `dom_id` (e.g. `player-0`, `word-1`) matches the
/// row's Transcript button. `subject_line` is prebuilt (already-escaped) HTML.
/// Reuses the shared `.modal-backdrop` (semi-transparent, page shows behind)
/// with the wider `.cm-audit-modal` card so the transcript reads cleanly.
fn audit_modal_html(
    dom_id: &str,
    action: &str,
    subject_line: &str,
    targeted: &[String],
    snapshot: &[ChatMessage],
) -> String {
    let action_e = escape(action);
    let snap = audit_snapshot_html(snapshot, targeted);
    format!(
        r#"<div id="cm-audit-back-{dom_id}" class="modal-backdrop cm-audit-back" onclick="if(event.target===this)bbCmAuditCloseAll()">
      <div class="modal-card cm-audit-modal" role="dialog" aria-modal="true" aria-label="Chat snapshot for {action_e}">
        <div class="cm-audit-modal-head">
          <div>
            <p class="section-title">Chat Snapshot &mdash; {action_e}</p>
            <p class="section-sub">{subject_line}</p>
          </div>
          <button type="button" class="cm-close" aria-label="Close snapshot" onclick="bbCmAuditCloseAll()">&#10005;</button>
        </div>
        <div class="cm-audit-modal-scroll">
          {snap}
        </div>
      </div>
    </div>"#
    )
}

/// Snapshot overlays for the Player table, keyed `player-{i}`.
fn player_audit_modals_html(entries: &[PlayerAuditEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.snapshot.is_empty())
        .map(|(i, e)| {
            let pid = PlayerId::from_counter(e.target_player_id);
            let subject = format!(
                r#"{tuser} <span class="cm-pid mono">ID {pid}</span> &middot; {bodies}"#,
                tuser = escape(&e.target_username),
                bodies = audit_bodies_inline(&e.body_ids),
            );
            audit_modal_html(&format!("player-{i}"), &e.action, &subject, &e.body_ids, &e.snapshot)
        })
        .collect()
}

/// Snapshot overlays for the Word table (Approve occurrences), keyed `word-{i}`.
fn word_audit_modals_html(entries: &[WordAuditEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.snapshot.is_empty())
        .map(|(i, e)| {
            let who = match (&e.target_username, e.target_player_id) {
                (Some(u), Some(id)) => format!(
                    r#"{u} <span class="cm-pid mono">ID {pid}</span>"#,
                    u = escape(u),
                    pid = PlayerId::from_counter(id),
                ),
                _ => r#"<span class="cm-audit-none">global</span>"#.to_string(),
            };
            let subject = format!(
                r#"Word: <span class="cm-flag">{word}</span> &middot; {who}"#,
                word = escape(&e.word),
            );
            audit_modal_html(&format!("word-{i}"), &e.action, &subject, &e.body_ids, &e.snapshot)
        })
        .collect()
}

/// Table-specific Advanced Filter fieldset bodies (inert placeholders).
const PLAYER_FILTER_SPECIFIC: &str = r#"<label>Player<input type="text" placeholder="Username or ID"></label>
          <label>Body ID<input type="text" placeholder="Body ID"></label>
          <label>Flagged word<input type="text" placeholder="Word"></label>"#;
const WORD_FILTER_SPECIFIC: &str = r#"<label>Word<input type="text" placeholder="Word"></label>
          <label>Player<input type="text" placeholder="Username or ID (Approve)"></label>"#;
const LIST_FILTER_SPECIFIC: &str = r#"<label>Player<input type="text" placeholder="Username or ID"></label>
          <label>List<select><option>Any</option><option>Ban List</option><option>Suspensions</option><option>Whitelist</option></select></label>"#;
const SYSTEM_FILTER_SPECIFIC: &str = r#"<label>Word<input type="text" placeholder="Word"></label>
          <label>Player<input type="text" placeholder="Username or ID"></label>
          <label>Source<input type="text" placeholder="Process (e.g. Word Filter)"></label>"#;

/// One category view: its Advanced Filter panel above its scrollable table.
fn audit_view_html(filter: String, table: String) -> String {
    format!(
        r#"{filter}
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// GET /admin/chatmod/audit — the Chat Audit Logs view. A dropdown selects which
/// category log renders: Player, Word, List, or System (automated) — each its own
/// table with direct headers and its own Advanced Filter (a shared spine +
/// table-specific fields). Player/Word-Approve/System rows carry a chat-snapshot
/// overlay opened from the Transcript button. `from` is the session the moderator
/// came from (resolved from `?from=`): it drives both the context-aware X target
/// (that session, else the landing page) and the Chat Nav links, which forward it
/// so hopping to Moderation Lists keeps the same context.
pub fn chatmod_audit_page(
    log: &AuditLog,
    sessions: &[ChatSession],
    from: Option<&str>,
    role: AdminRole,
    username: &str,
) -> String {
    let close_href = match from {
        Some(code) => format!("/admin/chatmod/session/{code}"),
        None => "/admin/chatmod".to_string(),
    };
    let close = escape(&close_href);
    let player_view = audit_view_html(
        audit_filter_panel(
            &["Warn Only", "Warn + Delete", "Suspend", "Ban"],
            PLAYER_FILTER_SPECIFIC,
        ),
        player_audit_table_html(&log.players),
    );
    let word_view = audit_view_html(
        audit_filter_panel(&["Blacklist Word", "Approve Word"], WORD_FILTER_SPECIFIC),
        word_audit_table_html(&log.words),
    );
    let list_view = audit_view_html(
        audit_filter_panel(
            &["Remove Ban", "Lift Suspension", "Whitelist Add", "Whitelist Remove"],
            LIST_FILTER_SPECIFIC,
        ),
        list_audit_table_html(&log.lists),
    );
    let system_view = audit_view_html(
        audit_filter_panel(&["Flag Word"], SYSTEM_FILTER_SPECIFIC),
        system_audit_table_html(&log.system),
    );
    let modals = format!(
        "{}{}{}",
        player_audit_modals_html(&log.players),
        word_audit_modals_html(&log.words),
        system_audit_modals_html(&log.system),
    );
    let center = format!(
        r#"{SUBHEAD_HTML}
<div class="cm-session-head">
  <div class="cm-session-headtext">
    <p class="cm-session-title">Chat Audit Logs</p>
    <p class="cm-session-sub">A ledger of moderation actions. Pick a log to view &mdash; each keeps its own columns and filters.</p>
  </div>
  <a href="{close}" class="cm-close" aria-label="Close audit logs">&#10005;</a>
</div>
<div class="cm-audit-select">
  <label for="cm-audit-cat">Log</label>
  <select id="cm-audit-cat" onchange="bbCmAuditCat(this)">
    <option value="player">Player Actions</option>
    <option value="word">Word Actions</option>
    <option value="list">List Actions</option>
    <option value="system">System (Automated)</option>
  </select>
</div>
<div class="cm-audit-view" data-cat="player">{player_view}</div>
<div class="cm-audit-view" data-cat="word" hidden>{word_view}</div>
<div class="cm-audit-view" data-cat="list" hidden>{list_view}</div>
<div class="cm-audit-view" data-cat="system" hidden>{system_view}</div>
{modals}"#
    );
    chatmod_shell(&center, sessions, None, from, ChatNavPage::Audit, role, username)
}

/// The **Backlisted Words** sub-tab: a three-column tools panel (add / remove /
/// CSV import) over the searchable "Words In List" ledger. Delete opens the
/// shared delete-confirm modal (reason required). Preview only — inert controls.
fn blacklist_panel_html(words: &[BlacklistWord]) -> String {
    let table = if words.is_empty() {
        r#"<p class="cm-empty">No blacklisted words.</p>"#.to_string()
    } else {
        let rows = words
            .iter()
            .map(|w| {
                let checked = if w.active_filter { " checked" } else { "" };
                let word = escape(&w.word);
                format!(
                    r#"<tr>
            <td>{word}</td>
            <td>{reason}</td>
            <td class="cm-lists-check"><input type="checkbox"{checked} aria-label="Active filter for {word}"></td>
            <td class="cm-lists-check"><button type="button" class="btn-trash" title="Delete" aria-label="Delete {word}" onclick="bbCmListsAsk('cm-lists-del-modal')">&#128465;</button></td>
          </tr>"#,
                    reason = escape(&w.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>Words</th><th>Reason Provided</th><th>Active Filter Toggle</th><th>Delete</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Add to Blacklist</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)" aria-label="Reason for adding">
    <textarea placeholder="Word or words &mdash; separate with ;" aria-label="Words to blacklist"></textarea>
    <button type="button" class="btn btn-sm">Confirm</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Remove From Blacklist</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)" aria-label="Reason for removing">
    <textarea placeholder="Word or words &mdash; separate with ;" aria-label="Words to remove"></textarea>
    <button type="button" class="btn btn-sm">Confirm</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Add Words From a CSV File</p>
    <label for="cm-lists-csv">Upload</label>
    <input type="file" id="cm-lists-csv" accept=".csv" aria-label="CSV file">
    <button type="button" class="btn btn-sm">Confirm</button>
  </div>
</div>
<p class="cm-lists-note">More than one word can be added at once &mdash; separate each with a <span class="mono">;</span> so the system counts them individually. Each word occupies its own row in the list below.</p>
<div class="cm-lists-search">
  <label for="cm-lists-word-search">Words In List</label>
  <input type="text" id="cm-lists-word-search" placeholder="Search &mdash; most relevant words rise to the top">
</div>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Banned Users** sub-tab: a *To Ban* / *Banned User Tools* panel over the
/// ban ledger. Ban and UnBan each open the shared confirm modal. Preview only.
fn banned_panel_html(banned: &[BannedUser]) -> String {
    let table = if banned.is_empty() {
        r#"<p class="cm-empty">No banned users.</p>"#.to_string()
    } else {
        let rows = banned
            .iter()
            .map(|b| {
                let pid = PlayerId::from_counter(b.player_id);
                let transcript = if b.has_transcript {
                    r#"<button type="button" class="btn btn-sm">Transcript</button>"#.to_string()
                } else {
                    r#"<span class="cm-audit-none">&mdash;</span>"#.to_string()
                };
                let user = escape(&b.username);
                format!(
                    r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{user}</td>
            <td><span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">{pid}</span></td>
            <td>{reason}</td>
            <td>{transcript}</td>
            <td class="cm-lists-check"><input type="checkbox" aria-label="Select {user}"></td>
          </tr>"#,
                    ts = escape(&b.timestamp),
                    reason = escape(&b.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>Timestamp</th><th>Username</th><th>User ID</th><th>Reason For Ban</th><th>Transcript</th><th>CheckBox</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">To Ban</p>
    <label for="cm-lists-ban-id">User ID</label>
    <input type="text" id="cm-lists-ban-id" placeholder="Player ID" aria-label="User ID to ban">
    <label for="cm-lists-ban-reason">Reason</label>
    <input type="text" id="cm-lists-ban-reason" placeholder="Reason (logged)">
    <label for="cm-lists-ban-words">Offensive Words</label>
    <input type="text" id="cm-lists-ban-words" placeholder="Word or words &mdash; separate with ;">
    <button type="button" class="btn btn-danger btn-sm" onclick="bbCmListsAsk('cm-lists-ban-modal')">Ban User</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Banned User Tools</p>
    <p class="cm-lists-hint">To un-ban: tick the checkbox of each user you want to reinstate, then press UnBan. You'll confirm with a required reason.</p>
    <button type="button" class="btn btn-sm" onclick="bbCmListsAsk('cm-lists-unban-modal')">UnBan</button>
  </div>
</div>
<p class="cm-lists-title">Banned Users</p>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Active Suspensions** sub-tab (the old standalone "Suspensions" nav item,
/// now folded in here): an *Extend / Clear / Suspend* tools panel over the
/// suspension ledger. Suspend-from-this-page is flagged under construction.
fn suspensions_panel_html(suspended: &[SuspendedUser]) -> String {
    let table = if suspended.is_empty() {
        r#"<p class="cm-empty">No active suspensions.</p>"#.to_string()
    } else {
        let rows = suspended
            .iter()
            .map(|s| {
                let pid = PlayerId::from_counter(s.player_id);
                let user = escape(&s.username);
                format!(
                    r#"<tr>
            <td class="cm-audit-ts mono">{ts}</td>
            <td>{user}</td>
            <td><span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">{pid}</span></td>
            <td>{dur}</td>
            <td>{rem}</td>
            <td>{reason}</td>
            <td class="cm-lists-check"><input type="checkbox" aria-label="Select {user}"></td>
          </tr>"#,
                    ts = escape(&s.timestamp),
                    dur = escape(&s.suspended_for),
                    rem = escape(&s.remaining),
                    reason = escape(&s.reason),
                )
            })
            .collect::<String>();
        format!(
            r#"<table class="cm-audit-table">
        <thead>
          <tr><th>TimeStamp</th><th>Username</th><th>UserID</th><th>Suspended For</th><th>Remaining Time Left</th><th>Reason</th><th>CheckBox</th></tr>
        </thead>
        <tbody>
          {rows}
        </tbody>
      </table>"#
        )
    };
    format!(
        r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Extend Current Suspension</p>
    <div class="row">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Days" aria-label="Days">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Hours" aria-label="Hours">
      <input type="text" class="cm-dur" inputmode="numeric" placeholder="Mins" aria-label="Minutes">
    </div>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Extend</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Clear Suspensions</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Clear</button>
  </div>
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Suspend User</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)">
    <button type="button" class="btn btn-sm">Suspend</button>
    <p class="note">Suspending a user from this page is under construction.</p>
  </div>
</div>
<p class="cm-lists-title">Suspended Users</p>
<div class="cm-audit-scroll">
  {table}
</div>"#
    )
}

/// The **Whitelisted Users** sub-tab — no mockup yet, so a placeholder panel.
fn whitelist_panel_html() -> String {
    r#"<div class="cm-lists-tools">
  <div class="cm-lists-tool">
    <p class="cm-lists-tool-title">Whitelisted Users</p>
    <p class="cm-lists-hint">Under construction &mdash; this list's layout lands in a later pass.</p>
  </div>
</div>
<p class="cm-empty">No whitelist design yet.</p>"#
        .to_string()
}

/// The three inert confirm dialogs the Moderation Lists tools open (delete word,
/// ban, un-ban). Reuse the shared `.modal-backdrop`/`.modal-card`; each `Confirm`
/// just closes for now (the wiring phase swaps in the real action).
const LISTS_MODALS_HTML: &str = r#"<div id="cm-lists-del-modal" class="modal-backdrop cm-lists-modal" onclick="if(event.target===this)bbCmListsCloseAll()">
  <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-lists-del-title" aria-describedby="cm-lists-del-desc">
    <p class="section-title" id="cm-lists-del-title">Remove Word From Blacklist</p>
    <p class="section-sub" id="cm-lists-del-desc">Removing a word requires a reason. It stops being filtered going forward.</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)" aria-label="Removal reason">
    <div class="modal-actions">
      <button type="button" class="btn btn-sm" onclick="bbCmListsCloseAll()">Cancel</button>
      <button type="button" class="btn btn-danger btn-sm" onclick="bbCmListsCloseAll()">Confirm</button>
    </div>
  </div>
</div>
<div id="cm-lists-ban-modal" class="modal-backdrop cm-lists-modal" onclick="if(event.target===this)bbCmListsCloseAll()">
  <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-lists-ban-title" aria-describedby="cm-lists-ban-desc">
    <p class="section-title" id="cm-lists-ban-title">Confirm Chat Ban</p>
    <p class="section-sub" id="cm-lists-ban-desc">Permanently remove this user's chat privileges? The player keeps playing; reversible only by un-banning here.</p>
    <div class="modal-actions">
      <button type="button" class="btn btn-sm" onclick="bbCmListsCloseAll()">Cancel</button>
      <button type="button" class="btn btn-danger btn-sm" onclick="bbCmListsCloseAll()">Confirm Chat Ban</button>
    </div>
  </div>
</div>
<div id="cm-lists-unban-modal" class="modal-backdrop cm-lists-modal" onclick="if(event.target===this)bbCmListsCloseAll()">
  <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-lists-unban-title" aria-describedby="cm-lists-unban-desc">
    <p class="section-title" id="cm-lists-unban-title">UnBan Selected Users</p>
    <p class="section-sub" id="cm-lists-unban-desc">Reinstates chat privileges for the ticked users. A reason is required.</p>
    <input type="text" class="cm-reason" placeholder="Reason (logged)" aria-label="Un-ban reason">
    <div class="modal-actions">
      <button type="button" class="btn btn-sm" onclick="bbCmListsCloseAll()">Cancel</button>
      <button type="button" class="btn btn-sm" onclick="bbCmListsCloseAll()">Confirm</button>
    </div>
  </div>
</div>"#;

/// GET /admin/chatmod/lists — the Moderation Lists view. A tab strip selects one
/// of four sub-tabs (Backlisted Words, Banned Users, Active Suspensions,
/// Whitelisted Users), each rendered as a tools panel over a list table; the tab
/// toggle is client-side (`bbCmListsTab`). `from` is the session the moderator
/// came from (resolved from `?from=`): it drives both the context-aware X target
/// (that session, else the landing page) and the Chat Nav links, which forward it
/// so hopping to Chat Audit Logs keeps the same context.
pub fn chatmod_lists_page(
    lists: &ModerationLists,
    sessions: &[ChatSession],
    from: Option<&str>,
    role: AdminRole,
    username: &str,
) -> String {
    let close_href = match from {
        Some(code) => format!("/admin/chatmod/session/{code}"),
        None => "/admin/chatmod".to_string(),
    };
    let close = escape(&close_href);
    let blacklist = blacklist_panel_html(&lists.blacklist);
    let banned = banned_panel_html(&lists.banned);
    let suspensions = suspensions_panel_html(&lists.suspended);
    let whitelist = whitelist_panel_html();
    let center = format!(
        r#"{SUBHEAD_HTML}
<div class="cm-session-head">
  <div class="cm-session-headtext">
    <p class="cm-session-title">Moderation Lists</p>
    <p class="cm-session-sub">Blacklisted words, banned users, active suspensions, and the whitelist &mdash; pick a list to manage.</p>
  </div>
  <a href="{close}" class="cm-close" aria-label="Close moderation lists">&#10005;</a>
</div>
<div class="cm-lists-tabs" role="tablist" aria-label="Moderation lists">
  <button type="button" class="cm-lists-tab cm-lists-tab-active" role="tab" aria-selected="true" data-tab="blacklist" onclick="bbCmListsTab(this)">Backlisted Words</button>
  <button type="button" class="cm-lists-tab" role="tab" aria-selected="false" data-tab="banned" onclick="bbCmListsTab(this)">Banned Users</button>
  <button type="button" class="cm-lists-tab" role="tab" aria-selected="false" data-tab="suspensions" onclick="bbCmListsTab(this)">Active Suspensions</button>
  <button type="button" class="cm-lists-tab" role="tab" aria-selected="false" data-tab="whitelist" onclick="bbCmListsTab(this)">Whitelisted Users</button>
</div>
<div class="cm-lists-panel" role="tabpanel" data-tab="blacklist">{blacklist}</div>
<div class="cm-lists-panel" role="tabpanel" data-tab="banned" hidden>{banned}</div>
<div class="cm-lists-panel" role="tabpanel" data-tab="suspensions" hidden>{suspensions}</div>
<div class="cm-lists-panel" role="tabpanel" data-tab="whitelist" hidden>{whitelist}</div>
{LISTS_MODALS_HTML}"#
    );
    chatmod_shell(&center, sessions, None, from, ChatNavPage::Lists, role, username)
}

#[cfg(test)]
mod tests {
    use super::{highlight, highlight_toggle};

    #[test]
    fn highlight_wraps_standalone_occurrences_only() {
        assert_eq!(
            highlight("scrub the scrubbing scrub", "scrub"),
            r#"<span class="cm-flag">scrub</span> the scrubbing <span class="cm-flag">scrub</span>"#
        );
    }

    #[test]
    fn highlight_never_matches_inside_escaped_entities() {
        // '&' escapes to &amp;. A flagged word equal to an entity fragment
        // must wrap only the real word, never corrupt the entity.
        assert_eq!(
            highlight("a & amp b", "amp"),
            r#"a &amp; <span class="cm-flag">amp</span> b"#
        );
    }

    #[test]
    fn highlight_escapes_body_markup_and_empty_word_is_noop() {
        assert_eq!(highlight("<b>x</b>", ""), "&lt;b&gt;x&lt;/b&gt;");
        assert_eq!(
            highlight("<i>frick</i>", "frick"),
            r#"&lt;i&gt;<span class="cm-flag">frick</span>&lt;/i&gt;"#
        );
    }

    #[test]
    fn highlight_toggle_emits_boundary_checked_buttons() {
        let html = highlight_toggle("frick and fricking", "frick");
        assert_eq!(html.matches("<button").count(), 1);
        assert!(html.contains("fricking"));
        assert!(html.contains(r#"data-word="frick""#));
    }
}
