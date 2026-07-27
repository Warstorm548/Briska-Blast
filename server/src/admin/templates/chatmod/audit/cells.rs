//! The leading table cells every Chat Audit Logs category shares — body ids,
//! the player identity pair, the Transcript control, and the actor's group.

use shared::types::player::PlayerId;

use super::super::super::common::escape;

/// The Body Id table cell: the single id (tap-to-copy), an em-dash for a
/// body-less action, or a `×N bodies` disclosure listing each id (all
/// tap-to-copy) when one entry covered several of that player's messages.
pub(super) fn audit_bodies_cell(body_ids: &[String]) -> String {
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
pub(super) fn audit_bodies_inline(body_ids: &[String]) -> String {
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
pub(super) fn audit_player_cells(username: Option<&str>, id: Option<u64>) -> String {
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
pub(super) fn audit_transcript_cell(dom_id: &str, has_snapshot: bool) -> String {
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
pub(super) fn audit_group_cell(group: &str) -> String {
    if group == "System" {
        r#"<span class="cm-audit-sys">System</span>"#.to_string()
    } else {
        escape(group)
    }
}
