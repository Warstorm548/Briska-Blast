//! The Chat Audit Logs page.

use super::fixtures::{sample_audit_log, sample_sessions};
use crate::admin::templates;

#[test]
fn audit_page_renders_table_and_snapshot_overlays() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(html.contains("Chat Audit Logs"));
    // A dropdown picks the category log; all four views are present.
    assert!(html.contains(r#"<select id="cm-audit-cat""#));
    for cat in ["player", "word", "list", "system"] {
        assert!(
            html.contains(&format!(r#"<div class="cm-audit-view" data-cat="{cat}""#)),
            "missing {cat} view"
        );
    }
    // Player table keeps its plain headers (Timestamp + Player ID are now
    // sortable controls, asserted separately below).
    for col in [
        "Display Name",
        "Group",
        "Action",
        "Reason",
        "Player UserName",
        "Body Id",
        "Flagged Words",
        "Transcript",
    ] {
        assert!(html.contains(&format!("<th>{col}</th>")), "missing player column {col}");
    }
    // Word/List tables bring their own direct headers.
    assert!(html.contains("<th>Word</th>"));
    assert!(html.contains("<th>List</th>"));
    // Timestamp and Player ID headers are sortable — a flip-arrow control
    // that reorders the rows (client-side).
    assert!(html.contains(
        r#"<th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button>"#
    ));
    assert!(html.contains(r#"class="cm-sort" onclick="bbCmSort(this)">Player ID<"#));
    // Each table's Advanced Filter has a shared spine group beside a
    // table-specific one (fieldset legends).
    assert!(html.contains("<legend>Applies to all logs</legend>"));
    assert!(html.contains("<legend>This table</legend>"));
    // The persistent spine carries a From/To time range (UTC) under the date
    // pickers, forced to a 24-hour clock via lang="en-GB".
    assert!(html.contains(r#"<label>From time (UTC)<input type="time" lang="en-GB"></label>"#));
    assert!(html.contains(r#"<label>To time (UTC)<input type="time" lang="en-GB"></label>"#));
    // Player rows: the mockup example — Warstorm/Admin warned EldenFire
    // (000000012) for a repeat offense; ids are tap-to-copy.
    assert!(html.contains("Repeat Offense"));
    assert!(html.contains(r#"data-copy="000000012""#));
    assert!(html.contains(r#"data-copy="T09XB4N6QW22""#));
    assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
    // A body-less action shows the em-dash placeholder.
    assert!(html.contains(r#"<span class="cm-audit-none">&mdash;</span>"#));
    // Two covered bodies condense into a ×N disclosure, not two rows.
    assert!(html.contains(r#"<details class="cm-audit-bodies">"#));
    assert!(html.contains("&times;2 bodies"));
    assert!(html.contains(r#"data-copy="L88KD3F1QA72""#));
    // Player Transcript opens that row's namespaced overlay; acted-on bodies
    // are tagged in the frozen snapshot.
    assert!(html.contains(r#"onclick="bbCmAuditOpen('player-0')""#));
    assert!(html.contains(r#"id="cm-audit-back-player-0""#));
    assert!(html.contains(r#"class="modal-card cm-audit-modal""#));
    assert!(html.contains(r#"<span class="cm-msg-tag">acted on</span>"#));
    // Word table: a global Blacklist (no snapshot) + an Approve occurrence
    // (index 1) which carries its own overlay.
    assert!(html.contains("Blacklist Word"));
    assert!(html.contains("Approve Word"));
    assert!(html.contains(r#"onclick="bbCmAuditOpen('word-1')""#));
    assert!(html.contains(r#"id="cm-audit-back-word-1""#));
    // List table: list-edit actions carry the list chip and no snapshot.
    assert!(html.contains("Remove Ban"));
    assert!(html.contains(r#"<span class="cm-audit-list">Banned Users</span>"#));
    // System (automated) actions carry the distinct Group=System badge.
    assert!(html.contains(r#"<span class="cm-audit-sys">System</span>"#));
    // Auto-enforcement on a player lands in the PLAYER log (Group=System),
    // not the System table.
    assert!(html.contains("Auto-Delete"));
    assert!(html.contains("Auto-Mod"));
    // The System table holds flag events (non-enforcement) with overlays.
    assert!(html.contains("Flag Word"));
    assert!(html.contains("Word Filter"));
    assert!(html.contains(r#"onclick="bbCmAuditOpen('system-0')""#));
    assert!(html.contains(r#"id="cm-audit-back-system-0""#));
    // On its own page, Chat Audit Logs is the active nav item.
    assert!(html.contains("cm-nav-current"));
}

#[test]
fn audit_splits_bulk_action_per_player_and_allows_recurrence() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    // One bulk Ban press over two players → two entries sharing a timestamp,
    // one per target player.
    assert_eq!(
        html.matches("2026-07-24 13:58:20 UTC").count(),
        2,
        "bulk ban should split into two same-timestamp rows"
    );
    assert!(html.contains("MossyOak"));
    // A player recurs across the session (EldenFire is targeted by Warn Only,
    // Warn + Delete, and Ban) — repeated actions are never merged.
    assert!(
        html.matches(r#"data-copy="000000012""#).count() >= 3,
        "EldenFire should appear as the target of several distinct actions"
    );
}

/// The HTML of one category view, sliced out of the rendered page.
///
/// The page emits all four views into `data-cat` divs and hides the inactive
/// ones client-side, so asserting on the whole document cannot tell which table
/// a row landed in — which is exactly what these tests are for.
fn view_of(html: &str, cat: &str) -> String {
    let marker = format!(r#"<div class="cm-audit-view" data-cat="{cat}""#);
    let start = html.find(&marker).unwrap_or_else(|| panic!("no {cat} view"));
    let rest = &html[start + marker.len()..];
    // Views are siblings, so the next one begins where this one ends. The last
    // view runs to the modals block that follows all four.
    let end = rest
        .find(r#"<div class="cm-audit-view""#)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// A record filed under the wrong category is invisible to a reviewer looking in
/// the obvious place, and nothing else in the page would reveal it — the tables
/// render whatever they are handed. So the split itself is asserted: a ban is a
/// player action, lifting one is a list edit, and neither leaks into the other.
#[test]
fn ban_and_unban_land_in_their_own_category_tables() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    let player_view = view_of(&html, "player");
    let list_view = view_of(&html, "list");

    assert!(player_view.contains("<td>Ban</td>"), "a ban is a player action");
    assert!(
        !player_view.contains("<td>Remove Ban</td>"),
        "lifting a ban is a list edit, not a player action"
    );
    assert!(list_view.contains("<td>Remove Ban</td>"));
    assert!(
        !list_view.contains("<td>Ban</td>"),
        "the ban itself must not also appear in the list log"
    );

    // A ban row carries everything a reviewer needs without opening anything:
    // why, the target, the cited words, and a way into the transcript.
    assert!(player_view.contains("Slur spam"));
    assert!(player_view.contains(r#"<span class="cm-flag">frick</span>"#));
    assert!(player_view.contains(r#"data-copy="000000012""#));
    assert!(player_view.contains("Transcript"));
}

/// The `full` flag is the whole difference between a ban's evidence and every
/// other record's. Asserted at the render boundary because that is where a
/// reviewer sees it: a truncated ban would look exactly like a complete one.
#[test]
fn only_a_ban_snapshot_shows_what_happened_after_the_action() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    // The ban overlay runs past the action and marks where it fell.
    assert!(html.contains("everything below happened afterwards"));
    assert!(
        html.contains("well that escalated"),
        "a ban's snapshot must keep the lines sent after it"
    );
    // Exactly one divider: the fixture's other records all end at their action,
    // so a second would mean a non-ban record had started rendering past its cut.
    assert_eq!(
        html.matches("everything below happened afterwards").count(),
        1,
        "only the ban record shows an action-point divider"
    );
}

#[test]
fn audit_close_target_follows_session_context() {
    // From the landing page there is no open session — the X returns there.
    let landing = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(landing.contains(r#"href="/admin/chatmod" class="cm-close""#));
    // Opened from inside a session, the X returns to that session view.
    let from_session = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        Some("FJ5B3V"),
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(from_session.contains(r#"href="/admin/chatmod/session/FJ5B3V" class="cm-close""#));
    // ...and the Moderation Lists nav link forwards the same session context,
    // so hopping between the two sub-pages doesn't drop it (regression: the
    // nav hrefs were built from the always-None active_code, not ?from=).
    assert!(from_session.contains(r#"href="/admin/chatmod/lists?from=FJ5B3V""#));
}
