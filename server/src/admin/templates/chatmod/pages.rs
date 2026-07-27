//! The two primary Chat-Mod views: the landing page (flagged-message overview)
//! and the entered-session view (transcript + Quick Access Tools).

use super::chrome::{ChatNavPage, SUBHEAD_HTML, TOOLS_HTML};
use super::model::{ChatMessage, ChatSession, FlaggedSession};
use super::panels::{flagged_list_html, transcript_html};
use super::shell::chatmod_shell;
use super::super::common::escape;
use crate::admin::AdminRole;

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
  <div class="cm-flag-scroll" id="cm-flagged">
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
    <div class="cm-chat-scroll" id="cm-chat">
      {messages}
    </div>
    <div class="cm-chatbar">
      <input type="text" id="cm-chatbar-input" placeholder="Moderator chat bar" maxlength="500" autocomplete="off" onkeydown="if(event.key==='Enter'){{event.preventDefault();bbCmSay();}}">
      <button type="button" class="btn btn-primary btn-sm" onclick="bbCmSay()">Send</button>
    </div>
  </div>
  {TOOLS_HTML}
</div>"#
    );
    chatmod_shell(&center, sessions, Some(code), Some(code), ChatNavPage::None, role, username)
}
