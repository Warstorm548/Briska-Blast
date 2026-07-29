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
        "",
        None,
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
    // are tagged in the frozen snapshot. Row 0 is the un-ban, which has no
    // conversation behind it — so the overlay belongs to row 1, and the index
    // being addressed per-row rather than assumed at 0 is the point.
    assert!(html.contains(r#"onclick="bbCmAuditOpen('player-1')""#));
    assert!(html.contains(r#"id="cm-audit-back-player-1""#));
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
    assert!(html.contains(r#"<span class="cm-audit-list">Ban List</span>"#));
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
        "",
        None,
    );
    // One bulk Ban press over two players → two entries sharing a timestamp,
    // one per target player. Counted within the Player view: a ban is also a
    // list edit, so the same two records appear again in the List view and a
    // whole-page count would find four.
    let player_view = view_of(&html, "player");
    assert_eq!(
        player_view.matches("2026-07-24 13:58:20 UTC").count(),
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

fn audit_page(log: &crate::admin::templates::AuditLog, range: &str, cat: Option<&str>) -> String {
    templates::chatmod_audit_page(
        log,
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
        range,
        cat,
    )
}

fn empty_log(window_label: &str, notice: Option<&str>) -> crate::admin::templates::AuditLog {
    crate::admin::templates::AuditLog {
        players: vec![],
        words: vec![],
        lists: vec![],
        system: vec![],
        window_label: window_label.into(),
        window_notice: notice.map(Into::into),
    }
}

/// The range must survive the round trip into the field, or correcting a typo
/// means retyping the whole thing — and the moderator cannot see what the page
/// actually acted on.
#[test]
fn the_submitted_range_is_echoed_back_into_the_field() {
    let html = audit_page(&sample_audit_log(), "100-200", None);
    assert!(html.contains(r#"name="range" value="100-200""#));
    // The form is a GET to the same page, so a range is shareable as a URL.
    assert!(html.contains(r#"<form class="cm-audit-filter" method="get" action="/admin/chatmod/audit">"#));
}

/// Every table is a window, so the window is stated even when rows come back.
#[test]
fn the_active_window_is_always_on_the_page() {
    let html = audit_page(&sample_audit_log(), "", None);
    assert!(html.contains("Showing records <strong>1-100</strong>"));
}

/// An empty table must name the window it searched. "No list actions recorded
/// yet" in front of someone who asked for records 400-500 is false — the actions
/// exist and are simply older than the request.
#[test]
fn an_empty_table_names_the_window_rather_than_claiming_nothing_happened() {
    let html = audit_page(&empty_log("400-500", None), "400-500", None);
    for kind in ["player actions", "word actions", "list actions", "automated actions"] {
        assert!(
            html.contains(&format!("No {kind} in records 400-500.")),
            "empty {kind} table should name the window"
        );
    }
    assert!(!html.contains("recorded yet"), "the old unqualified wording must be gone");
}

/// A rejected or clamped range that applied silently would be read as the answer
/// to the question that was asked.
#[test]
fn a_range_that_was_not_honoured_says_so() {
    let html = audit_page(&empty_log("1-100", Some("That isn't a valid range")), "200-100", None);
    assert!(html.contains(r#"<p class="cm-audit-window-notice">That isn't a valid range</p>"#));

    // The rejected input is still echoed into the field so it can be corrected,
    // and markup in it must not survive into the page.
    let hostile = audit_page(&empty_log("1-100", Some("nope")), r#""><script>x</script>"#, None);
    assert!(!hostile.contains("<script>x</script>"), "range input must be escaped");

    // Matched on the element, not the bare class — the stylesheet is inlined
    // into every page, so the class name alone is always present.
    let clean = audit_page(&sample_audit_log(), "", None);
    assert!(
        !clean.contains(r#"<p class="cm-audit-window-notice">"#),
        "an honoured range needs no notice"
    );
}

/// Reset clears the range, not the moderator's context. Dropping `from` would
/// leave the X close and the Chat Nav links pointing at the landing page instead
/// of the session they came from.
#[test]
fn reset_keeps_the_session_context_and_the_open_log() {
    let with_context = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        Some("FJ5B3V"),
        crate::admin::AdminRole::Moderator,
        "modtester",
        "100-200",
        Some("list"),
    );
    assert!(with_context.contains(r#"href="/admin/chatmod/audit?cat=list&amp;from=FJ5B3V" class="btn">Reset</a>"#));

    // With no session to preserve, Reset still returns to the same log.
    let plain = audit_page(&sample_audit_log(), "100-200", Some("word"));
    assert!(plain.contains(r#"href="/admin/chatmod/audit?cat=word" class="btn">Reset</a>"#));
}

/// Submitting a range from the List table must come back to the List table.
/// Without this the form navigates and drops the moderator on Player, which is
/// the same class of bug as the session poll wiping transcript ticks.
#[test]
fn the_submitted_tab_stays_open_across_the_round_trip() {
    let html = audit_page(&sample_audit_log(), "1-50", Some("list"));
    assert!(html.contains(r#"<option value="list" selected>List Actions</option>"#));
    assert!(html.contains(r#"<div class="cm-audit-view" data-cat="list">"#));
    // ...and the others are the hidden ones.
    assert!(html.contains(r#"<div class="cm-audit-view" data-cat="player" hidden>"#));

    // Each panel carries its own category, so the round trip knows where it came from.
    assert!(html.contains(r#"<input type="hidden" name="cat" value="list">"#));

    // An unknown category falls back to Player rather than hiding every view.
    let junk = audit_page(&sample_audit_log(), "", Some("nonsense"));
    assert!(junk.contains(r#"<option value="player" selected>Player Actions</option>"#));
    assert!(junk.contains(r#"<div class="cm-audit-view" data-cat="player">"#));
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

/// A ban and an un-ban are each **one record shown in two tables** — an action
/// on a player that also edits the ban list. Asserted at the render boundary
/// because that is where the model is either honoured or lost: the tables render
/// whatever they are handed, so a record missing from a view is invisible to a
/// reviewer looking in the obvious place and nothing else on the page reveals
/// it.
///
/// The previous invariant was the opposite — ban in Player only, un-ban in List
/// only — which left a player's reversals out of the log any per-player total
/// has to be counted from. That is the gap this replaces.
#[test]
fn bans_and_unbans_appear_in_both_the_player_and_list_tables() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
        "",
        None,
    );
    let player_view = view_of(&html, "player");
    let list_view = view_of(&html, "list");

    for view in [&player_view, &list_view] {
        assert!(view.contains("<td>Ban</td>"), "a ban is both a player action and a list edit");
        assert!(
            view.contains("<td>Remove Ban</td>"),
            "lifting a ban is both a player action and a list edit"
        );
    }

    // Only the List table names which list was touched; the Player table has no
    // such column, which is what lets one record serve both without either view
    // showing a field that means nothing to it.
    assert!(list_view.contains(r#"<span class="cm-audit-list">Ban List</span>"#));
    assert!(!player_view.contains("cm-audit-list"));

    // A ban row carries everything a reviewer needs without opening anything:
    // why, the target, the cited words, and a way into the transcript.
    assert!(player_view.contains("Slur spam"));
    assert!(player_view.contains(r#"<span class="cm-flag">frick</span>"#));
    assert!(player_view.contains(r#"data-copy="000000012""#));
    assert!(player_view.contains("Transcript"));
}

/// The tag is what puts a row in the List view, so an action that edits no list
/// must stay out of it. Without this, "everything lands in both tables" would
/// pass the test above just as well as the actual rule does.
#[test]
fn warnings_stay_out_of_the_list_table() {
    let html = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
        "",
        None,
    );
    let player_view = view_of(&html, "player");
    let list_view = view_of(&html, "list");

    assert!(player_view.contains("<td>Warn + Delete</td>"));
    assert!(
        !list_view.contains("<td>Warn + Delete</td>"),
        "a warning edits no list and must not appear in the list log"
    );
    assert!(
        !list_view.contains("<td>Auto-Delete</td>"),
        "automated enforcement edits no list either"
    );
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
        "",
        None,
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
        "",
        None,
    );
    assert!(landing.contains(r#"href="/admin/chatmod" class="cm-close""#));
    // Opened from inside a session, the X returns to that session view.
    let from_session = templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        Some("FJ5B3V"),
        crate::admin::AdminRole::Moderator,
        "modtester",
        "",
        None,
    );
    assert!(from_session.contains(r#"href="/admin/chatmod/session/FJ5B3V" class="cm-close""#));
    // ...and the Moderation Lists nav link forwards the same session context,
    // so hopping between the two sub-pages doesn't drop it (regression: the
    // nav hrefs were built from the always-None active_code, not ?from=).
    assert!(from_session.contains(r#"href="/admin/chatmod/lists?from=FJ5B3V""#));
}
