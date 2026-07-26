//! The **Moderation Lists** page: four sub-tabs over the panels in
//! [`panels`], plus the confirm dialogs their controls open.

mod panels;

use super::chrome::{ChatNavPage, SUBHEAD_HTML};
use super::model::{ChatSession, ModerationLists};
use super::shell::chatmod_shell;
use super::super::common::escape;
use crate::admin::AdminRole;

use panels::{
    banned_panel_html, blacklist_panel_html, suspensions_panel_html, whitelist_panel_html,
};

/// The three inert confirm dialogs the Moderation Lists tools open (delete word,
/// ban, un-ban). Reuse the shared `.modal-backdrop`/`.modal-card`; each `Confirm`
/// just closes for now (the wiring phase swaps in the real action).
const LISTS_MODALS_HTML: &str = r#"<div id="cm-lists-del-modal" class="modal-backdrop cm-lists-modal" onclick="if(event.target===this)bbCmListsCloseAll()">
  <div class="modal-card" role="alertdialog" aria-modal="true" aria-labelledby="cm-lists-del-title" aria-describedby="cm-lists-del-desc">
    <p class="section-title" id="cm-lists-del-title">Remove Word From Blacklist</p>
    <p class="section-sub" id="cm-lists-del-desc">Removing a word requires a reason. It stops being filtered going forward.</p>
    <p class="section-sub">Word: <span class="mono" id="cm-lists-del-who"></span></p>
    <input type="text" class="cm-reason" id="cm-lists-del-why" placeholder="Reason (logged)" aria-label="Removal reason">
    <div class="modal-actions">
      <button type="button" class="btn btn-sm" onclick="bbCmListsCloseAll()">Cancel</button>
      <button type="button" class="btn btn-danger btn-sm" onclick="bbCmListsDeleteConfirm()">Confirm</button>
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
    notice: Option<(bool, &str)>,
    role: AdminRole,
    username: &str,
) -> String {
    let close_href = match from {
        Some(code) => format!("/admin/chatmod/session/{code}"),
        None => "/admin/chatmod".to_string(),
    };
    let close = escape(&close_href);
    // Same `?ok=` / `?err=` banner shape the Users and Dashboard tabs use.
    let notice_html = match notice {
        Some((true, text)) => format!(r#"<div class="msg-ok">&#10003; {}</div>"#, escape(text)),
        Some((false, text)) => format!(r#"<div class="msg-err">&#10007; {}</div>"#, escape(text)),
        None => String::new(),
    };
    let blacklist = blacklist_panel_html(&lists.blacklist, from);
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
{notice_html}
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
