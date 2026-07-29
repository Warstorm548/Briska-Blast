//! The chat-snapshot overlays behind each audit row's Transcript button, and
//! the replayed transcript they show.

use shared::types::player::PlayerId;

use super::super::super::common::escape;
use super::super::highlight::highlight;
use super::super::model::{
    message_role, ChatMessage, MessageRole, PlayerAuditEntry, SystemAuditEntry, WordAuditEntry,
};
use super::super::panels::posted_as_html;
use super::cells::audit_bodies_inline;

/// Snapshot overlays for the System table, keyed `system-{i}`.
pub(super) fn system_audit_modals_html(entries: &[SystemAuditEntry]) -> String {
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
            audit_modal_html(&format!("system-{i}"), &e.action, &subject, &e.body_ids, &e.snapshot, None)
        })
        .collect()
}

/// Read-only snapshot rows for the audit overlay: the live transcript's
/// message-card look without the select checkboxes or tappable flag toggles —
/// a frozen record, so blacklisted words use the static red span. Bodies in
/// `targeted` (the entry's `body_ids`) are flagged as "acted on" so the
/// moderator sees exactly which messages the action covered.
///
/// `cut` marks where the action fell for the records that show the whole
/// conversation rather than stopping at it (bans). Without the divider a
/// reviewer would read messages sent *after* the ban as evidence that led to it.
pub(in crate::admin::templates::chatmod) fn audit_snapshot_html(
    snapshot: &[ChatMessage],
    targeted: &[String],
    cut: Option<usize>,
) -> String {
    if snapshot.is_empty() {
        return r#"<p class="cm-empty">No chat captured for this action.</p>"#.to_string();
    }
    snapshot
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let divider = if cut == Some(i) {
                r#"<p class="cm-audit-cut">action taken here &mdash; everything below happened afterwards</p>"#
            } else {
                ""
            };
            format!("{divider}{}", snapshot_row_html(m, targeted))
        })
        .collect()
}

/// One message card inside a snapshot overlay.
fn snapshot_row_html(m: &ChatMessage, targeted: &[String]) -> String {
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
    // A ban or warning echo names the moderator's action on a player, not
    // something they said — tagged so it never reads as the player's own line.
    let mod_tag = match message_role(m) {
        MessageRole::Moderator => r#"<span class="cm-msg-mod">MOD</span> "#,
        MessageRole::Warning => r#"<span class="cm-msg-warn">WARNED</span> "#,
        MessageRole::Ban => r#"<span class="cm-msg-ban">BANNED</span> "#,
        MessageRole::Player => "",
    };
    format!(
        r#"<div class="cm-msg{cls}">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{mod_tag}{user}{posted_as} {pid_chip}<span class="cm-bodyid mono">Body ID: {id}</span>{tag}</span>
            </div>
            <div class="cm-msg-body">{body}</div>
          </div>"#,
        id = escape(&m.body_id),
        user = escape(&m.username),
        posted_as = posted_as_html(m),
    )
}

/// One hidden snapshot overlay. `dom_id` (e.g. `player-0`, `word-1`) matches the
/// row's Transcript button. `subject_line` is prebuilt (already-escaped) HTML.
/// Reuses the shared `.modal-backdrop` (semi-transparent, page shows behind)
/// with the wider `.cm-audit-modal` card so the transcript reads cleanly.
///
/// `cut` divides what prompted the action from what followed it, for the records
/// that keep the whole conversation. Public to the chatmod templates because the
/// Banned Users ledger opens the same overlay — a second snapshot renderer would
/// drift from this one.
pub(in crate::admin::templates::chatmod) fn audit_modal_html(
    dom_id: &str,
    action: &str,
    subject_line: &str,
    targeted: &[String],
    snapshot: &[ChatMessage],
    cut: Option<usize>,
) -> String {
    let action_e = escape(action);
    let snap = audit_snapshot_html(snapshot, targeted, cut);
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
pub(super) fn player_audit_modals_html(entries: &[PlayerAuditEntry]) -> String {
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
            audit_modal_html(
                &format!("player-{i}"),
                &e.action,
                &subject,
                &e.body_ids,
                &e.snapshot,
                e.snapshot_cut,
            )
        })
        .collect()
}

/// Snapshot overlays for the Word table (Approve occurrences), keyed `word-{i}`.
pub(super) fn word_audit_modals_html(entries: &[WordAuditEntry]) -> String {
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
            audit_modal_html(&format!("word-{i}"), &e.action, &subject, &e.body_ids, &e.snapshot, None)
        })
        .collect()
}
