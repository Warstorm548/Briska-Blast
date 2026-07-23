//! The admin **Chat-Mod** tab — the chat-moderation panel (UI preview).
//!
//! Layout phase only: both views render placeholder data supplied by the
//! handler (`admin::chatmod`) so the design can be iterated in a browser before
//! any wiring. Two server-rendered views share one full-width three-column
//! shell — the landing page (flagged-message overview) and the session view
//! (transcript + quick-access tools). This is the first tab to span the full
//! viewport width instead of the shared `.page` container; everything here is
//! `cm-`-prefixed so it can't collide with the common stylesheet.

use super::common::{escape, nav_html, CSS};
use crate::admin::AdminRole;

/// One entry in the left "Active Game Sessions" panel.
pub struct ChatSession {
    pub code: String,
    /// The last few chat lines, newest last — the card's preview text.
    pub preview: Vec<String>,
    /// True when the session has system-flagged messages (red-dot badge).
    pub flagged: bool,
}

/// A flagged message body inside a [`FlaggedSession`].
pub struct FlaggedBody {
    /// Message-body identifier (server-assigned 12-char alphanumeric once
    /// wired; never shown to game players — moderation surfaces only).
    pub body_id: String,
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
    pub body: String,
    /// Present when the body contains a blacklisted word to highlight.
    pub flagged_word: Option<String>,
}

/// Page-specific styles, appended after the shared `{CSS}`. A plain const
/// (rather than text inside `format!`) so none of the braces need doubling.
const CHATMOD_CSS: &str = "
/* Chat-Mod: full-width three-column moderation layout */
.cm-page { width: 100%; }
.cm-card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 24px; }
.cm-layout { display: grid; grid-template-columns: 280px minmax(0, 1fr) 260px; gap: 16px; padding-top: 14px; }
.cm-panel { background: #0d1117; border: 1px solid #30363d; border-radius: 8px; padding: 14px; }
.cm-panel-title { font-size: 0.7rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: #8b949e; padding-bottom: 8px; margin-bottom: 10px; border-bottom: 1px solid #30363d; }
.cm-empty { font-size: 0.82rem; color: #6e7681; text-align: center; padding: 14px 0; }
/* left panel: session cards */
.cm-session-card { position: relative; display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 10px 12px; margin-bottom: 10px; text-decoration: none; color: #c9d1d9; }
.cm-session-card:hover { border-color: #8b949e; }
.cm-session-card.cm-session-active { border-color: #e94560; }
.cm-session-code { font-size: 0.72rem; color: #8b949e; margin-bottom: 4px; }
.cm-session-preview { font-size: 0.76rem; color: #6e7681; line-height: 1.5; overflow-wrap: anywhere; }
.cm-dot { position: absolute; top: 9px; right: 9px; width: 10px; height: 10px; border-radius: 50%; background: #f85149; }
/* center column */
.cm-center { min-width: 0; }
.cm-subhead { display: flex; align-items: center; justify-content: space-between; gap: 10px; border: 1px solid #30363d; border-radius: 8px; padding: 8px 10px; margin-bottom: 14px; }
.cm-subhead-title { flex: 1; text-align: center; font-size: 1.05rem; font-weight: 600; }
.cm-burger { display: none; cursor: pointer; font-size: 1.25rem; line-height: 1; color: #c9d1d9; background: none; border: none; padding: 2px 8px; }
/* landing: flagged messages */
.cm-flag-wrap { border: 1px solid #30363d; border-radius: 8px; padding: 18px 16px; min-height: 62vh; }
.cm-flag-title { text-align: center; font-size: 1.25rem; font-weight: 700; margin-bottom: 4px; }
.cm-flag-sub { text-align: center; font-size: 0.8rem; color: #8b949e; margin-bottom: 14px; }
.cm-flag-block { border: 1px solid #30363d; border-radius: 8px; padding: 14px; min-height: 48vh; }
.cm-flag-card { display: block; background: #161b22; border: 1px solid #30363d; border-radius: 10px; padding: 10px 14px; margin-bottom: 12px; text-decoration: none; color: #c9d1d9; }
.cm-flag-card:hover { border-color: #8b949e; }
.cm-flag-code { font-size: 0.85rem; font-weight: 600; margin-bottom: 6px; }
.cm-flag-body { font-size: 0.82rem; margin-bottom: 2px; overflow-wrap: anywhere; }
.cm-bodyid { font-size: 0.7rem; color: #6e7681; margin-bottom: 6px; }
.cm-flag { background: rgba(248, 81, 73, 0.18); color: #f85149; border-radius: 3px; padding: 0 3px; }
/* session view */
.cm-session-head { text-align: center; margin-bottom: 12px; }
.cm-session-title { font-size: 1.25rem; font-weight: 700; margin-bottom: 4px; }
.cm-session-sub { font-size: 0.8rem; color: #8b949e; }
.cm-session-frame { position: relative; border: 1px solid #30363d; border-radius: 8px; padding: 14px; padding-top: 36px; }
.cm-close { position: absolute; top: 8px; right: 10px; color: #8b949e; text-decoration: none; font-size: 1.05rem; line-height: 1; padding: 4px 8px; border-radius: 6px; }
.cm-close:hover { color: #f85149; background: #2d1117; }
.cm-session-body { display: grid; grid-template-columns: minmax(0, 2fr) minmax(240px, 1fr); gap: 16px; }
.cm-chat { display: flex; flex-direction: column; border: 1px solid #30363d; border-radius: 14px; padding: 14px; min-height: 56vh; }
.cm-chat-scroll { flex: 1; overflow-y: auto; max-height: 60vh; }
.cm-msg { background: #161b22; border: 1px solid #30363d; border-radius: 10px; padding: 10px 12px; margin-bottom: 10px; }
.cm-msg-head { display: flex; align-items: center; gap: 10px; font-size: 0.7rem; color: #8b949e; margin-bottom: 6px; }
.cm-msg-user { flex: 1; }
.cm-msg-body { font-size: 0.85rem; overflow-wrap: anywhere; }
.cm-chatbar { display: flex; gap: 8px; margin-top: 12px; }
/* quick access tools */
.cm-tool { margin-bottom: 14px; }
.cm-tool-btn { width: 100%; }
.cm-tool-settings { margin-top: 18px; }
.cm-check { display: flex; align-items: center; gap: 8px; font-size: 0.84rem; color: #c9d1d9; }
/* right panel: placeholder secondary nav */
.cm-nav-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 9px 8px; font-size: 0.85rem; color: #8b949e; border-bottom: 1px solid #21262d; cursor: not-allowed; opacity: 0.65; }
.cm-soon { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 1px; color: #6e7681; border: 1px solid #30363d; border-radius: 10px; padding: 1px 7px; }
/* mobile: side panels become edge drawers toggled by the corner burgers */
.cm-backdrop { display: none; position: fixed; inset: 0; background: rgba(1, 4, 9, 0.6); z-index: 90; }
@media (max-width: 768px) {
  .cm-card { padding: 20px 16px; }
  .cm-layout { grid-template-columns: 1fr; }
  .cm-burger { display: inline-block; }
  .cm-left, .cm-right { position: fixed; top: 0; height: 100vh; width: 280px; max-width: 85vw; overflow-y: auto; border-radius: 0; background: #161b22; z-index: 100; visibility: hidden; transition: transform .2s ease, visibility 0s linear .2s; }
  .cm-left { left: 0; border-right: 1px solid #30363d; transform: translateX(-100%); }
  .cm-right { right: 0; border-left: 1px solid #30363d; transform: translateX(100%); }
  body.cm-left-open .cm-left, body.cm-right-open .cm-right { transform: translateX(0); visibility: visible; transition: transform .2s ease; }
  body.cm-left-open .cm-backdrop, body.cm-right-open .cm-backdrop { display: block; }
  .cm-session-body { grid-template-columns: 1fr; }
  .cm-chat { min-height: 46vh; }
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
        <span class="cm-nav-item">Player Whitelist<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Banned List<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Suspensions<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Chat Audit Logs<span class="cm-soon">soon</span></span>
        <span class="cm-nav-item">Mod User Settings<span class="cm-soon">soon</span></span>"#;

/// The session view's "Quick Access Tools" panel. Deliberately styled as the
/// finished controls (not grayed out) so the final look can be judged — but
/// nothing is wired: buttons are `type="button"` with no handlers.
const TOOLS_HTML: &str = r#"<div class="cm-panel cm-tools">
      <p class="cm-panel-title">Quick Access Tools</p>
      <div class="cm-tool">
        <button type="button" class="btn btn-sm cm-tool-btn">Warn + Delete Chat Body</button>
      </div>
      <div class="cm-tool">
        <label>Suspend User</label>
        <div class="row">
          <input type="text" placeholder="days:hours:mins">
          <button type="button" class="btn btn-sm">Suspend</button>
        </div>
      </div>
      <div class="cm-tool">
        <label>Ban User</label>
        <div class="row">
          <input type="text" placeholder="Reason">
          <button type="button" class="btn btn-danger btn-sm">Ban</button>
        </div>
      </div>
      <div class="cm-tool">
        <label>Blacklist Word</label>
        <div class="row">
          <input type="text" placeholder="Word or phrase">
          <button type="button" class="btn btn-sm">Add</button>
        </div>
      </div>
      <div class="cm-tool">
        <button type="button" class="btn btn-sm cm-tool-btn">Approve Word</button>
      </div>
      <div class="cm-tool cm-tool-settings">
        <p class="cm-panel-title">Moderator Chat Settings</p>
        <label class="cm-check"><input type="checkbox"> Appear As Your Display Name</label>
      </div>
      <p class="note">Preview only &mdash; tools are not wired up yet.</p>
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
                .map(|line| escape(line))
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
                        r#"<div class="cm-flag-body">{body}</div>
          <div class="cm-bodyid mono">Body ID: {id}</div>"#,
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
                Some(word) => highlight(&m.body, word),
                None => escape(&m.body),
            };
            format!(
                r#"<div class="cm-msg">
            <div class="cm-msg-head">
              <span class="mono">Body ID: {id}</span>
              <span class="cm-msg-user">{user}</span>
              <input type="checkbox" aria-label="Select message {id}">
            </div>
            <div class="cm-msg-body">{body}</div>
          </div>"#,
                id = escape(&m.body_id),
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
        {session_cards}
      </aside>
      <main class="cm-center">
        {center}
      </main>
      <aside class="cm-panel cm-right" id="cm-right">
        <p class="cm-panel-title">Chat Nav</p>
        {chat_nav}
      </aside>
    </div>
    <div class="cm-backdrop" onclick="bbCmClose()"></div>
  </div>
</div>
<script>
function bbCmToggle(btn,side){{var cls=side==='left'?'cm-left-open':'cm-right-open';document.body.classList.remove(side==='left'?'cm-right-open':'cm-left-open');var open=document.body.classList.toggle(cls);btn.setAttribute('aria-expanded',open?'true':'false');}}
function bbCmClose(){{document.body.classList.remove('cm-left-open','cm-right-open');document.querySelectorAll('.cm-burger').forEach(function(b){{b.setAttribute('aria-expanded','false');}});}}
document.addEventListener('keydown',function(e){{if(e.key==='Escape')bbCmClose();}});
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
  <div class="cm-flag-block">
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
  <p class="cm-session-title">Session Chat Code: {code_html}, You Have Entered</p>
  <p class="cm-session-sub">You can monitor, join the chat when necessary, and use mod tools at your discretion.</p>
</div>
<div class="cm-session-frame">
  <a href="/admin/chatmod" class="cm-close" aria-label="Leave session">&#10005;</a>
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
  </div>
</div>"#
    );
    chatmod_shell(&center, sessions, Some(code), role, username)
}
