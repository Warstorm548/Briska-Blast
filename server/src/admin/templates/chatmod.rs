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
    pub player_id: u64,
    pub body: String,
    /// Present when the body contains a blacklisted word to highlight.
    pub flagged_word: Option<String>,
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
.cm-close { color: #8b949e; text-decoration: none; font-size: 1.05rem; line-height: 1; padding: 4px 8px; border-radius: 6px; }
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
";

/// The center sub-header shared by both views: title bar with a burger at each
/// corner. The burgers are hidden ≥768px; below that they slide the left
/// (sessions) / right (chat nav) panels in as drawers.
const SUBHEAD_HTML: &str = r#"<div class="cm-subhead">
  <button type="button" class="cm-burger" aria-label="Open sessions panel" aria-controls="cm-left" aria-expanded="false" onclick="bbCmToggle(this,'left')">&#9776;</button>
  <span class="cm-subhead-title">Chat Moderation Area</span>
  <button type="button" class="cm-burger" aria-label="Open chat nav panel" aria-controls="cm-right" aria-expanded="false" onclick="bbCmToggle(this,'right')">&#9776;</button>
</div>"#;

/// The right-hand "Chat Nav" secondary navigation. Every entry is a visual
/// placeholder until its sub-page is designed — rendered inert on purpose.
const CHAT_NAV_HTML: &str = r#"<span class="cm-nav-item">Settings<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Moderation Lists<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Suspensions<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Chat Audit Logs<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Mod User Settings<span class="cm-soon">soon</span></span>"#;

/// The session view's "Quick Access Tools" panel. Deliberately styled as the
/// finished controls (not grayed out) so the final look can be judged — but
/// nothing is wired: buttons are `type="button"` with no handlers.
const TOOLS_HTML: &str = r#"<div class="cm-panel cm-tools">
      <p class="cm-panel-title">Quick Access Tools</p>
      <div class="cm-panel-scroll">
      <div class="cm-tool-group">
        <p class="cm-tool-group-title">Player Actions</p>
        <div class="cm-tool">
          <label>Target Player IDs</label>
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
          <label>Blacklist Words</label>
          <div class="row">
            <input type="text" placeholder="Word or words &mdash; separate with ;">
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

/// Escape `body`, then wrap the (escaped) blacklisted `word` in the red
/// highlight span. Replacement happens strictly after escaping so the span is
/// the only markup that survives — user text can never smuggle in HTML.
fn highlight(body: &str, word: &str) -> String {
    let escaped = escape(body);
    if word.is_empty() {
        return escaped;
    }
    let needle = escape(word);
    escaped.replace(&needle, &format!(r#"<span class="cm-flag">{needle}</span>"#))
}

/// Transcript variant of [`highlight`]: each occurrence renders as an inline
/// toggle button so moderators can tap words to build the Approve selection.
/// Per-instance by default; the "Select all matching words" checkbox in the
/// tools panel widens a tap to every occurrence of that word (`data-word`).
fn highlight_toggle(body: &str, word: &str) -> String {
    let escaped = escape(body);
    if word.is_empty() {
        return escaped;
    }
    let needle = escape(word);
    escaped.replace(
        &needle,
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
            format!(
                r#"<div class="cm-msg">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{user} <span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span> <span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></span>
              <input type="checkbox" data-pid="{pid}" aria-label="Select message {id}">
            </div>
            <div class="cm-msg-body">{body}</div>
          </div>"#,
                id = escape(&m.body_id),
                user = escape(&m.username),
                pid = PlayerId::from_counter(m.player_id),
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
    role: AdminRole,
    username: &str,
) -> String {
    let nav = nav_html("chatmod", role, username);
    let session_cards = session_list_html(sessions, active_code);
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
</script>
</body>
</html>"#,
        chat_nav = CHAT_NAV_HTML,
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
    chatmod_shell(&center, sessions, None, role, username)
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
    chatmod_shell(&center, sessions, Some(code), role, username)
}
