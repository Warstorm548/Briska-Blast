//! Blacklisted-word highlighting inside message bodies — the static red span
//! used in previews and snapshots, and the tappable toggle used in the live
//! transcript. Both escape as they go, so a match can never land inside an
//! HTML entity produced by escaping.

use super::super::common::escape;

/// Render `body` with every standalone occurrence of `word` replaced by the
/// prebuilt `wrapped` markup. Matching runs on the raw (unescaped) text with
/// word-boundary checks — an adjacent alphanumeric disqualifies a match, so
/// "scrub" never fires inside "scrubbing" — and each non-match segment is
/// escaped separately, so a match can never land inside an HTML entity
/// produced by escaping. Case-sensitive; the wiring phase's flagging engine
/// owns smarter matching and will hand over exact occurrences.
fn highlight_with(body: &str, word: &str, wrapped: &str) -> String {
    if word.is_empty() {
        return escape(body);
    }
    let mut out = String::new();
    let mut rest = body;
    while let Some(idx) = rest.find(word) {
        let end = idx + word.len();
        let boundary_before = rest[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let boundary_after = rest[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if boundary_before && boundary_after {
            out.push_str(&escape(&rest[..idx]));
            out.push_str(wrapped);
        } else {
            // Substring of a larger word — pass it through untouched.
            out.push_str(&escape(&rest[..end]));
        }
        rest = &rest[end..];
    }
    out.push_str(&escape(rest));
    out
}

/// Wrap standalone occurrences of the blacklisted `word` in the red
/// highlight span (see [`highlight_with`] for the matching rules).
pub(super) fn highlight(body: &str, word: &str) -> String {
    let needle = escape(word);
    highlight_with(
        body,
        word,
        &format!(r#"<span class="cm-flag">{needle}</span>"#),
    )
}

/// Transcript variant of [`highlight`]: each occurrence renders as an inline
/// toggle button so moderators can tap words to build the Approve selection.
/// Per-instance by default; the "Select all matching words" checkbox in the
/// tools panel widens a tap to every occurrence of that word (`data-word`).
pub(super) fn highlight_toggle(body: &str, word: &str) -> String {
    let needle = escape(word);
    highlight_with(
        body,
        word,
        &format!(
            r#"<button type="button" class="cm-flag cm-flag-btn" data-word="{needle}" aria-pressed="false" onclick="bbCmFlagToggle(this)">{needle}</button>"#
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{highlight, highlight_toggle};

    #[test]
    fn highlight_wraps_standalone_occurrences_only() {
        assert_eq!(
            highlight("scrub the scrubbing scrub", "scrub"),
            r#"<span class="cm-flag">scrub</span> the scrubbing <span class="cm-flag">scrub</span>"#
        );
    }

    #[test]
    fn highlight_never_matches_inside_escaped_entities() {
        // '&' escapes to &amp;. A flagged word equal to an entity fragment
        // must wrap only the real word, never corrupt the entity.
        assert_eq!(
            highlight("a & amp b", "amp"),
            r#"a &amp; <span class="cm-flag">amp</span> b"#
        );
    }

    #[test]
    fn highlight_escapes_body_markup_and_empty_word_is_noop() {
        assert_eq!(highlight("<b>x</b>", ""), "&lt;b&gt;x&lt;/b&gt;");
        assert_eq!(
            highlight("<i>frick</i>", "frick"),
            r#"&lt;i&gt;<span class="cm-flag">frick</span>&lt;/i&gt;"#
        );
    }

    #[test]
    fn highlight_toggle_emits_boundary_checked_buttons() {
        let html = highlight_toggle("frick and fricking", "frick");
        assert_eq!(html.matches("<button").count(), 1);
        assert!(html.contains("fricking"));
        assert!(html.contains(r#"data-word="frick""#));
    }
}
