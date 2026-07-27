//! The player tools: how a warn reports what it did and did not reach.

use super::super::player::warn_summary;

#[test]
fn warn_summary_names_who_it_missed() {
    let one = |s: &str| vec![s.to_string()];

    assert_eq!(warn_summary(&one("Warstorm (000000007)"), &[], 0), "Warned 1 player.");
    assert_eq!(
        warn_summary(&["Warstorm (000000007)".into(), "EldenFire (000000012)".into()], &[], 0),
        "Warned 2 players."
    );

    // The case this exists for. A warning is not queued, so a moderator who is
    // told only "warned 1 player" would have no way to know the other target
    // never saw it — and would reasonably assume the message landed.
    assert_eq!(
        warn_summary(
            &one("Warstorm (000000007)"),
            &one("EldenFire (000000012) — not connected"),
            0
        ),
        "Warned 1 player. Not delivered to EldenFire (000000012) — not connected."
    );

    // Reaching nobody must not read as a success.
    assert_eq!(
        warn_summary(&[], &one("EldenFire (000000012) — in an active match"), 0),
        "Warned nobody — EldenFire (000000012) — in an active match."
    );
    assert_eq!(warn_summary(&[], &[], 0), "Nobody to warn.");
}

#[test]
fn warn_summary_reports_deletions_alongside_delivery() {
    let one = vec!["Warstorm (000000007)".to_string()];

    assert_eq!(warn_summary(&one, &[], 1), "Warned 1 player. 1 message removed.");
    assert_eq!(warn_summary(&one, &[], 3), "Warned 1 player. 3 messages removed.");

    // Deletion and delivery succeed independently: the bodies are withdrawn from
    // everyone still connected even when the sender themselves has gone. Saying
    // so matters, because otherwise a moderator retries an action that worked.
    assert_eq!(
        warn_summary(&[], &["Warstorm (000000007) — not connected".to_string()], 2),
        "Warned nobody — Warstorm (000000007) — not connected. 2 messages removed."
    );

    // Partial delivery *and* a deletion — the branch a real Warn + Delete on two
    // players most often lands in. The count belongs between the delivery and
    // failure sentences, so the removal does not read as something that only
    // happened to the player it reached.
    assert_eq!(
        warn_summary(
            &one,
            &["EldenFire (000000012) — not connected".to_string()],
            2
        ),
        "Warned 1 player. 2 messages removed. Not delivered to EldenFire (000000012) — not connected."
    );

    // Warn Only passes 0 and must not mention messages at all.
    assert_eq!(warn_summary(&one, &[], 0), "Warned 1 player.");
}

#[test]
fn a_target_who_was_never_in_the_session_is_reported_not_acted_on() {
    // Strangers are filtered out before anything is written, so they can only
    // ever appear in the missed list. One typo in the ;-separated target field
    // would otherwise leave a permanent audit record against an uninvolved
    // player, and manual deletion of audit records is not built.
    assert_eq!(
        warn_summary(&[], &["000000999 — not in this session".to_string()], 0),
        "Warned nobody — 000000999 — not in this session."
    );
}
