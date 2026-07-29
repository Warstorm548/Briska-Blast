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
    range: &str,
    cat: Option<&str>,
) -> String {
    let close_href = match from {
        Some(code) => format!("/admin/chatmod/session/{code}"),
        None => "/admin/chatmod".to_string(),
    };
    let close = escape(&close_href);
    // Raw here: each table escapes it into its own empty state.
    let window = log.window_label.as_str();
    let player_view = audit_view_html(
        audit_filter_panel(
            "player",
            &["Warn Only", "Warn + Delete", "Suspend", "Ban"],
            PLAYER_FILTER_SPECIFIC,
            range,
            from,
        ),
        player_audit_table_html(&log.players, window),
    );
    let word_view = audit_view_html(
        audit_filter_panel(
            "word",
            &["Blacklist Word", "Approve Word"],
            WORD_FILTER_SPECIFIC,
            range,
            from,
        ),
        word_audit_table_html(&log.words, window),
    );
    // A ban and an un-ban are player actions that also edit the ban list, so
    // they are offered here as well — filtering this table for `Ban` must find
    // the rows it shows.
    let list_view = audit_view_html(
        audit_filter_panel(
            "list",
            &[
                "Ban",
                "Remove Ban",
                "Lift Suspension",
                "Whitelist Add",
                "Whitelist Remove",
            ],
            LIST_FILTER_SPECIFIC,
            range,
            from,
        ),
        list_audit_table_html(&log.lists, window),
    );
    let system_view = audit_view_html(
        audit_filter_panel("system", &["Flag Word"], SYSTEM_FILTER_SPECIFIC, range, from),
        system_audit_table_html(&log.system, window),
    );
    let modals = format!(
        "{}{}{}",
        player_audit_modals_html(&log.players),
        word_audit_modals_html(&log.words),
        system_audit_modals_html(&log.system),
    );
    // Which log the moderator was on when they submitted. Unknown values fall
    // back to Player rather than rendering four hidden views and a blank page.
    let active = match cat {
        Some("word") => "word",
        Some("list") => "list",
        Some("system") => "system",
        _ => "player",
    };
    let selected = |name: &str| if name == active { " selected" } else { "" };
    let hidden = |name: &str| if name == active { "" } else { " hidden" };

    let window_shown = escape(window);
    // A rejected or clamped range must be visible: a moderator who typed
    // `200-100` and silently got the newest 100 would read the result as the
    // answer to their question.
    let window_notice = match log.window_notice.as_deref() {
        Some(msg) => format!(r#"<p class="cm-audit-window-notice">{}</p>"#, escape(msg)),
        None => String::new(),
    };

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
    <option value="player"{sel_player}>Player Actions</option>
    <option value="word"{sel_word}>Word Actions</option>
    <option value="list"{sel_list}>List Actions</option>
    <option value="system"{sel_system}>System (Automated)</option>
  </select>
  <p class="cm-audit-window">Showing records <strong>{window_shown}</strong> of each log, newest first.</p>
</div>
{window_notice}
<div class="cm-audit-view" data-cat="player"{hide_player}>{player_view}</div>
<div class="cm-audit-view" data-cat="word"{hide_word}>{word_view}</div>
<div class="cm-audit-view" data-cat="list"{hide_list}>{list_view}</div>
<div class="cm-audit-view" data-cat="system"{hide_system}>{system_view}</div>
{modals}"#,
        sel_player = selected("player"),
        sel_word = selected("word"),
        sel_list = selected("list"),
        sel_system = selected("system"),
        hide_player = hidden("player"),
        hide_word = hidden("word"),
        hide_list = hidden("list"),
        hide_system = hidden("system"),
    );
    chatmod_shell(&center, sessions, None, from, ChatNavPage::Audit, role, username)
}
