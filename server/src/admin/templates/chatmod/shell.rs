//! The full-width three-column document every Chat-Mod view is rendered into.

use super::super::common::{escape, nav_html, CSS};
use super::chrome::{chat_nav_html, ChatNavPage};
use super::model::ChatSession;
use super::panels::session_list_html;
use super::script::CHATMOD_JS;
use super::style::CHATMOD_CSS;
use crate::admin::AdminRole;

/// The full-width three-column document both views share: nav, left sessions
/// panel, injected `center`, right Chat Nav panel, and the drawer-toggle
/// script. The script itself is interpolated from [`CHATMOD_JS`] rather than
/// written inline, so its braces stay single — see [`super::script`].
pub(super) fn chatmod_shell(
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
    // The live-refresh poller reads this to decide what to ask for: inside a
    // session it wants that transcript, elsewhere just the landing panels.
    let body_attrs = match active_code {
        Some(code) => format!(r#" data-cm-code="{}""#, escape(code)),
        None => String::new(),
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Briska Blast — Admin Chat-Mod</title>
  <style>{CSS}{CHATMOD_CSS}</style>
</head>
<body{body_attrs}>
<div class="cm-page">
  <div class="cm-card">
    {nav}
    <div class="cm-layout">
      <aside class="cm-panel cm-left" id="cm-left">
        <p class="cm-panel-title">Active Game Sessions</p>
        <div class="cm-panel-scroll" id="cm-sessions">
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
{CHATMOD_JS}
</script>
</body>
</html>"#
    )
}
