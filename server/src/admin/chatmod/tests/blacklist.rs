//! The Blacklisted Words tools: word splitting, the duplicate summary, and the
//! delete control's escaping.

use super::super::blacklist::{add_summary, split_words};
use super::fixtures::sample_sessions;
use crate::admin::templates::{self, BlacklistWord, ModerationLists};

#[test]
fn a_word_containing_a_quote_does_not_break_the_delete_control() {
    // Regression: the delete button used to interpolate the word straight
    // into an inline JS call. `escape` does not touch single quotes, so
    // "don't" terminated the string literal and broke the whole control.
    let lists = ModerationLists {
        blacklist: vec![BlacklistWord {
            word: "don't".into(),
            reason: "Apostrophe check".into(),
            active_filter: true,
        }],
        banned: vec![],
        suspended: vec![],
    };
    let html = templates::chatmod_lists_page(
        &lists,
        &sample_sessions(),
        None,
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(
        html.contains(r#"data-word="don't" onclick="bbCmListsDelete(this.getAttribute('data-word'))""#),
        "the word must travel in an attribute, not an inline JS argument"
    );
    assert!(
        !html.contains("bbCmListsDelete('don't')"),
        "must never emit an unterminated JS string literal"
    );
}

#[test]
fn add_summary_names_the_words_that_were_already_listed() {
    let w = |s: &str| vec![s.to_string()];

    assert_eq!(add_summary(&w("frick"), &[]), "Added 1 word.");
    assert_eq!(
        add_summary(&["frick".into(), "scrub".into()], &[]),
        "Added 2 words."
    );
    // The case this exists for: a partial add used to just report a smaller
    // count, leaving the moderator to work out which word didn't take.
    assert_eq!(
        add_summary(&["frick".into(), "scrub".into()], &w("chicken")),
        "Added 2 words. 1 already listed: chicken."
    );
    assert_eq!(
        add_summary(&w("frick"), &["chicken".into(), "scrub".into()]),
        "Added 1 word. 2 already listed: chicken, scrub."
    );
    // Nothing changed — the banner must not read as a success.
    assert_eq!(
        add_summary(&[], &w("chicken")),
        "No new words — it is already listed: chicken."
    );
    assert_eq!(
        add_summary(&[], &["chicken".into(), "scrub".into()]),
        "No new words — they are already listed: chicken, scrub."
    );
    assert_eq!(add_summary(&[], &[]), "Nothing to add.");
}

#[test]
fn words_split_on_semicolons_and_drop_blanks() {
    assert_eq!(split_words("frick;scrub"), vec!["frick", "scrub"]);
    // The panel tells moderators to separate with `;`, and people add spaces.
    assert_eq!(split_words(" frick ; scrub "), vec!["frick", "scrub"]);
    assert_eq!(split_words("frick;;scrub;"), vec!["frick", "scrub"]);
    assert_eq!(split_words(""), Vec::<String>::new());
    assert_eq!(split_words("  ;  ; "), Vec::<String>::new());
    // A single word needs no separator.
    assert_eq!(split_words("frick"), vec!["frick"]);
}
