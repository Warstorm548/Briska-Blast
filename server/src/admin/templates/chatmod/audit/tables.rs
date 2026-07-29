//! One table per audit category. All four open with the same "who / when /
//! what / why" columns, then add their own subject/evidence columns.

use super::super::super::common::escape;
use super::super::model::{ListAuditEntry, PlayerAuditEntry, SystemAuditEntry, WordAuditEntry};
use super::cells::{audit_bodies_cell, audit_group_cell, audit_player_cells, audit_transcript_cell};

/// What an empty table says.
///
/// Names the window rather than stopping at "nothing here", because every table
/// is a *slice* of its log. "No list actions recorded yet" in front of a
/// moderator who asked for records 400-500 is simply false — the actions exist,
/// they are older than what was requested — and it is the kind of false that
/// ends an investigation early.
fn empty_state(kind: &str, window: &str) -> String {
    format!(
        r#"<p class="cm-empty">No {kind} in records {window}. Widen the range to look further back.</p>"#,
        kind = escape(kind),
        window = escape(window),
    )
}

/// **Player** category table (today's headers, unchanged): one row per
/// (action, target player); a player's covered bodies condense into a
/// `×N bodies` disclosure, flagged words render as red chips. A row's actor may
/// be the program (`Group = System`, an auto-enforcement rule) — rendered
/// identically but with the System badge.
pub(super) fn player_audit_table_html(entries: &[PlayerAuditEntry], window: &str) -> String {
    if entries.is_empty() {
        return empty_state("player actions", window);
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
pub(super) fn word_audit_table_html(entries: &[WordAuditEntry], window: &str) -> String {
    if entries.is_empty() {
        return empty_state("word actions", window);
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
pub(super) fn list_audit_table_html(entries: &[ListAuditEntry], window: &str) -> String {
    if entries.is_empty() {
        return empty_state("list actions", window);
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
pub(super) fn system_audit_table_html(entries: &[SystemAuditEntry], window: &str) -> String {
    if entries.is_empty() {
        return empty_state("automated actions", window);
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
