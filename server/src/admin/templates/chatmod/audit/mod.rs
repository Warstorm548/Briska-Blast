//! The **Chat Audit Logs** page: a category dropdown over four record tables,
//! each with its own Advanced Filter panel and snapshot overlays.

mod cells;
mod filters;
pub(super) mod modals;
mod tables;

use super::chrome::{ChatNavPage, SUBHEAD_HTML};
use super::model::{AuditLog, ChatSession};
use super::shell::chatmod_shell;
use super::super::common::escape;
use crate::admin::AdminRole;

use filters::{
    audit_filter_panel, LIST_FILTER_SPECIFIC, PLAYER_FILTER_SPECIFIC, SYSTEM_FILTER_SPECIFIC,
    WORD_FILTER_SPECIFIC,
};
use modals::{
    player_audit_modals_html, system_audit_modals_html, word_audit_modals_html,
};
use tables::{
    list_audit_table_html, player_audit_table_html, system_audit_table_html,
    word_audit_table_html,
};

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
