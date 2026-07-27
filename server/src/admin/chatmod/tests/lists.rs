//! The Moderation Lists page.

use super::fixtures::{sample_moderation_lists, sample_sessions};
use crate::admin::templates;

#[test]
fn lists_page_renders_subtabs_and_tables() {
    let html = templates::chatmod_lists_page(
        &sample_moderation_lists(),
        &sample_sessions(),
        None,
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(html.contains("Moderation Lists"));
    // Four sub-tabs, one panel each; Backlisted Words is selected by default.
    for (tab, label) in [
        ("blacklist", "Backlisted Words"),
        ("banned", "Banned Users"),
        ("suspensions", "Active Suspensions"),
        ("whitelist", "Whitelisted Users"),
    ] {
        assert!(html.contains(&format!(r#"data-tab="{tab}""#)), "missing {tab} tab/panel");
        assert!(html.contains(label), "missing {label} label");
    }
    assert!(html.contains(
        r#"class="cm-lists-tab cm-lists-tab-active" role="tab" aria-selected="true" data-tab="blacklist""#
    ));
    // Backlisted Words: the three tools + the ledger's four columns.
    assert!(html.contains("Add to Blacklist"));
    assert!(html.contains("Remove From Blacklist"));
    assert!(html.contains("Add Words From a CSV File"));
    for col in ["Words", "Reason Provided", "Active Filter Toggle", "Delete"] {
        assert!(html.contains(&format!("<th>{col}</th>")), "missing blacklist column {col}");
    }
    // The trash button names the word in the confirm dialog, so a moderator
    // sees exactly what they are about to remove; only Confirm submits. The
    // word travels in a data attribute, not an inline JS argument, because
    // `escape` leaves single quotes alone — a word like "don't" would
    // otherwise terminate the string literal.
    assert!(html.contains(r#"data-word="frick" onclick="bbCmListsDelete(this.getAttribute('data-word'))""#));
    assert!(html.contains(r#"id="cm-lists-del-modal""#));
    assert!(html.contains(r#"onclick="bbCmListsDeleteConfirm()""#));
    // Add / Remove are real posts, not inert buttons.
    assert!(html.contains(r#"action="/admin/chatmod/lists/blacklist/add""#));
    assert!(html.contains(r#"action="/admin/chatmod/lists/blacklist/remove""#));
    assert!(html.contains(r#"<textarea name="words""#));
    assert!(html.contains(r#"name="reason" placeholder="Reason (logged)""#));
    // The Active Filter toggle posts the value it moves TO, so a stale page
    // can't flip a word the opposite way from what the moderator saw. In the
    // fixture "chicken" is inactive and the other two are active.
    assert!(html.contains(r#"action="/admin/chatmod/lists/blacklist/toggle""#));
    assert_eq!(
        html.matches(r#"<input type="hidden" name="active" value="0">"#).count(),
        2,
        "the two active words should offer to turn OFF"
    );
    assert_eq!(
        html.matches(r#"<input type="hidden" name="active" value="1">"#).count(),
        1,
        "the inactive word should offer to turn ON"
    );
    // Banned Users columns + the ban/unban confirm modals.
    for col in ["Timestamp", "Username", "User ID", "Reason For Ban", "Transcript", "CheckBox"] {
        assert!(html.contains(&format!("<th>{col}</th>")), "missing banned column {col}");
    }
    assert!(html.contains(r#"onclick="bbCmListsAsk('cm-lists-ban-modal')""#));
    assert!(html.contains(r#"onclick="bbCmListsAsk('cm-lists-unban-modal')""#));
    // Banned rows show the canonical zero-padded player id, tap-to-copy.
    assert!(html.contains(r#"data-copy="000000012""#));
    // Active Suspensions columns + the three duration fields.
    for col in ["TimeStamp", "Suspended For", "Remaining Time Left"] {
        assert!(html.contains(&format!("<th>{col}</th>")), "missing suspension column {col}");
    }
    for ph in ["Days", "Hours", "Mins"] {
        assert!(html.contains(&format!(
            r#"class="cm-dur" inputmode="numeric" placeholder="{ph}""#
        )));
    }
    assert!(html.contains("Suspending a user from this page is under construction."));
    // On its own page, Moderation Lists is the active Chat Nav item.
    assert!(html.contains(r#"cm-nav-current" aria-current="page">Moderation Lists</span>"#));
}

#[test]
fn lists_close_target_follows_session_context() {
    // From the landing there is no open session — the X returns there.
    let landing = templates::chatmod_lists_page(
        &sample_moderation_lists(),
        &sample_sessions(),
        None,
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(landing.contains(r#"href="/admin/chatmod" class="cm-close""#));
    // Opened from inside a session, the X returns to that session view.
    let from_session = templates::chatmod_lists_page(
        &sample_moderation_lists(),
        &sample_sessions(),
        Some("FJ5B3V"),
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(from_session.contains(r#"href="/admin/chatmod/session/FJ5B3V" class="cm-close""#));
    // ...and the Chat Audit Logs nav link forwards the same session context.
    assert!(from_session.contains(r#"href="/admin/chatmod/audit?from=FJ5B3V""#));
}
