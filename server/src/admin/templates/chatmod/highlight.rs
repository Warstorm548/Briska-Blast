//! Blacklisted-word highlighting inside message bodies — the static red span
//! used in previews and snapshots, and the tappable toggle used in the live
//! transcript. Both escape as they go, so a match can never land inside an
//! HTML entity produced by escaping.
//!
//! Matching is **not** reimplemented here. It runs through
//! [`crate::chat::blacklist::find_matches`] — the same engine that masks the
//! word before broadcast — so what a moderator sees marked red and what a
//! player saw replaced by `#` agree by construction. A hand-rolled matcher
//! lived here once and drifted: it was case-sensitive against a word list that
//! is stored ASCII-lowercased, so a capitalised occurrence was censored
//! in-game and highlighted nowhere.

use crate::chat::blacklist::find_matches;

use super::super::common::escape;

/// Render `body` with every blacklisted occurrence replaced by `wrap`'s markup.
///
/// `wrap` receives the matched text **as the player typed it** and the
/// normalized blacklist word that fired, in that order — the first is what a
/// moderator should read, the second is the stable identity used for grouping.
///
/// Matching runs on the raw (unescaped) text and each non-match segment is
/// escaped separately, so a match can never land inside an HTML entity produced
/// by escaping. An empty word list escapes the whole body and nothing else.
fn highlight_with(body: &str, words: &[String], wrap: impl Fn(&str, &str) -> String) -> String {
    let matches = find_matches(body, words);
    if matches.is_empty() {
        return escape(body);
    }
    let mut out = String::with_capacity(body.len());
    let mut cursor = 0;
    for m in &matches {
        let end = m.start + m.len;
        out.push_str(&escape(&body[cursor..m.start]));
        out.push_str(&wrap(&body[m.start..end], &m.word));
        cursor = end;
    }
    out.push_str(&escape(&body[cursor..]));
    out
}

/// Wrap every blacklisted occurrence in the red highlight span.
pub(super) fn highlight(body: &str, words: &[String]) -> String {
    highlight_with(body, words, |text, _word| {
        format!(r#"<span class="cm-flag">{}</span>"#, escape(text))
    })
}

/// Transcript variant of [`highlight`]: each occurrence renders as an inline
/// toggle button so moderators can tap words to build the Approve selection.
/// Per-instance by default; the "Select all matching words" checkbox in the
/// tools panel widens a tap to every occurrence of that word (`data-word`).
///
/// `data-word` carries the *normalized* word rather than the typed text, so
/// `Frick` and `frick` widen to one another instead of forming two groups.
pub(super) fn highlight_toggle(body: &str, words: &[String]) -> String {
    highlight_with(body, words, |text, word| {
        format!(
            r#"<button type="button" class="cm-flag cm-flag-btn" data-word="{word}" aria-pressed="false" onclick="bbCmFlagToggle(this)">{text}</button>"#,
            word = escape(word),
            text = escape(text),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{highlight, highlight_toggle};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn highlight_wraps_standalone_occurrences_only() {
        assert_eq!(
            highlight("scrub the scrubbing scrub", &words(&["scrub"])),
            r#"<span class="cm-flag">scrub</span> the scrubbing <span class="cm-flag">scrub</span>"#
        );
    }

    #[test]
    fn highlight_never_matches_inside_escaped_entities() {
        // '&' escapes to &amp;. A flagged word equal to an entity fragment
        // must wrap only the real word, never corrupt the entity.
        assert_eq!(
            highlight("a & amp b", &words(&["amp"])),
            r#"a &amp; <span class="cm-flag">amp</span> b"#
        );
    }

    #[test]
    fn highlight_escapes_body_markup_and_empty_list_is_noop() {
        assert_eq!(highlight("<b>x</b>", &[]), "&lt;b&gt;x&lt;/b&gt;");
        assert_eq!(
            highlight("<i>frick</i>", &words(&["frick"])),
            r#"&lt;i&gt;<span class="cm-flag">frick</span>&lt;/i&gt;"#
        );
    }

    #[test]
    fn highlight_matches_regardless_of_case() {
        // The reported bug: the stored word list is ASCII-lowercased, so a
        // capitalised occurrence was masked in-game and highlighted nowhere.
        // The label keeps the sender's casing — a moderator reads what was said.
        assert_eq!(
            highlight("FRICK you", &words(&["frick"])),
            r#"<span class="cm-flag">FRICK</span> you"#
        );
        assert_eq!(
            highlight("FrIcK you", &words(&["frick"])),
            r#"<span class="cm-flag">FrIcK</span> you"#
        );
    }

    #[test]
    fn highlight_marks_every_distinct_word_in_one_body() {
        // The other half of the bug: only the first flagged word ever rendered.
        let html = highlight("frick you scrub, frick off", &words(&["frick", "scrub"]));
        assert_eq!(html.matches(r#"<span class="cm-flag">"#).count(), 3);
        assert!(html.contains(r#"<span class="cm-flag">scrub</span>"#));
    }

    #[test]
    fn highlight_collapses_overlapping_words_like_the_censor_does() {
        // find_matches resolves overlaps (longest wins), so nested markup is
        // impossible — "bad" inside "badword" must not open a second span.
        let html = highlight("badword here", &words(&["bad", "badword"]));
        assert_eq!(html, r#"<span class="cm-flag">badword</span> here"#);
    }

    #[test]
    fn highlight_preserves_surrounding_multi_byte_text() {
        assert_eq!(
            highlight("héllo 🎮 frick 🎮 wörld", &words(&["frick"])),
            r#"héllo 🎮 <span class="cm-flag">frick</span> 🎮 wörld"#
        );
    }

    #[test]
    fn highlight_toggle_emits_boundary_checked_buttons() {
        let html = highlight_toggle("frick and fricking", &words(&["frick"]));
        assert_eq!(html.matches("<button").count(), 1);
        assert!(html.contains("fricking"));
        assert!(html.contains(r#"data-word="frick""#));
    }

    #[test]
    fn highlight_toggle_groups_case_variants_under_one_word() {
        // The label shows what was typed; data-word stays normalized so the
        // "select all matching words" checkbox treats both as one word.
        let html = highlight_toggle("FRICK and frick", &words(&["frick"]));
        assert_eq!(html.matches(r#"data-word="frick""#).count(), 2);
        assert!(html.contains(">FRICK</button>"));
        assert!(html.contains(">frick</button>"));
    }
}
