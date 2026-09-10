//! Keep-a-Changelog markdown → per-version entries.
//!
//! Both `GameChangeLog.md` and `LauncherChangeLog.md` use one shape, verified
//! against every `## ` line in both files:
//!
//! ```text
//! ## [0.35.1] — 2026-09-08
//! <body…>
//!
//! ---
//!
//! ## [0.35.0] — 2026-09-07
//! ```
//!
//! Two details that are easy to get wrong and silently drop every entry:
//! the separator is an **em dash (U+2014)**, not a hyphen, and each body is
//! terminated by a `---` horizontal rule that belongs to the file's formatting
//! rather than to the entry. Both files also open with a preamble before the
//! first heading, which is not part of any entry.

use semver::Version;

/// One released version's changelog section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub version: Version,
    /// The heading's date, e.g. `2026-09-08`. Empty when the heading carried
    /// no date — displayed as-is, never parsed.
    pub date: String,
    /// The section body as markdown, with the surrounding blank lines and the
    /// trailing `---` rule removed.
    pub body: String,
}

/// Parse a changelog into entries, newest version first.
///
/// Anything before the first recognisable heading (title, format note, rename
/// banner) is dropped. Sorting is by parsed semver rather than trusting file
/// order, so a hand-edit that puts a version out of place still renders in the
/// right sequence.
pub fn parse(markdown: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut current: Option<(Version, String, Vec<&str>)> = None;

    for line in markdown.lines() {
        if let Some((version, date)) = parse_heading(line) {
            if let Some((v, d, body)) = current.take() {
                entries.push(Entry {
                    version: v,
                    date: d,
                    body: clean_body(&body),
                });
            }
            current = Some((version, date, Vec::new()));
        } else if let Some((_, _, body)) = current.as_mut() {
            body.push(line);
        }
        // Lines before the first heading are the file preamble — skipped.
    }
    if let Some((v, d, body)) = current.take() {
        entries.push(Entry {
            version: v,
            date: d,
            body: clean_body(&body),
        });
    }

    entries.sort_by(|a, b| b.version.cmp(&a.version));
    entries
}

/// Recognise `## [<semver>] — <date>`, returning the version and date text.
///
/// The trailing separator is accepted as an em dash, en dash or hyphen: the
/// files use an em dash today, and being lenient here means a future typo
/// costs a missing date rather than a silently missing entry. A heading whose
/// bracketed text is not valid semver (e.g. `## [Unreleased]`) returns `None`
/// and its section is folded into the previous entry's body, which is the
/// correct handling for a section that describes no released version.
fn parse_heading(line: &str) -> Option<(Version, String)> {
    let rest = line.strip_prefix("## [")?;
    let (version_text, after) = rest.split_once(']')?;
    let version = Version::parse(version_text.trim()).ok()?;
    let date = after
        .trim_start()
        .trim_start_matches(['\u{2014}', '\u{2013}', '-'])
        .trim()
        .to_string();
    Some((version, date))
}

/// Trim a collected body: drop blank padding at both ends, then drop the
/// trailing `---` rule that separates entries in the source file (and the blank
/// padding that rule leaves behind).
fn clean_body(lines: &[&str]) -> String {
    let mut end = lines.len();
    let mut start = 0;

    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    // The rule belongs to the file's layout, not to this entry's content.
    if end > start && is_rule(lines[end - 1]) {
        end -= 1;
        while end > start && lines[end - 1].trim().is_empty() {
            end -= 1;
        }
    }

    lines[start..end].join("\n")
}

/// A markdown thematic break made of dashes (`---`, `----`, …). Deliberately
/// narrow: `***` and `___` are also valid rules in markdown but these files
/// only ever use dashes, and matching more risks eating real content.
fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.chars().all(|c| c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real file shape: preamble, em-dash headings, `---` separators.
    const SAMPLE: &str = "\
# Game Changelog

Some preamble prose.

---

## [0.35.1] — 2026-09-08

Fixed the thing.

---

## [0.35.0] — 2026-09-07

Added the other thing.

---

## [0.34.1] — 2026-08-26

Older entry.
";

    #[test]
    fn parses_entries_newest_first() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].version, Version::parse("0.35.1").unwrap());
        assert_eq!(entries[1].version, Version::parse("0.35.0").unwrap());
        assert_eq!(entries[2].version, Version::parse("0.34.1").unwrap());
    }

    #[test]
    fn captures_the_heading_date() {
        let entries = parse(SAMPLE);
        assert_eq!(entries[0].date, "2026-09-08");
        assert_eq!(entries[2].date, "2026-08-26");
    }

    /// The preamble must never leak into the first entry, and the `---`
    /// separator must never leak into the previous one.
    #[test]
    fn body_excludes_preamble_and_separator() {
        let entries = parse(SAMPLE);
        assert_eq!(entries[0].body, "Fixed the thing.");
        assert_eq!(entries[1].body, "Added the other thing.");
        // Last entry has no trailing rule at all.
        assert_eq!(entries[2].body, "Older entry.");
        assert!(!entries[0].body.contains("preamble"));
    }

    #[test]
    fn multi_line_bodies_keep_internal_blank_lines() {
        let md = "## [1.0.0] — 2026-01-01\n\nFirst para.\n\nSecond para.\n\n---\n";
        let entries = parse(md);
        assert_eq!(entries[0].body, "First para.\n\nSecond para.");
    }

    /// A hyphen or en dash instead of the em dash still yields the entry.
    #[test]
    fn tolerates_other_dash_separators() {
        for sep in ["\u{2014}", "\u{2013}", "-"] {
            let md = format!("## [1.2.3] {sep} 2026-05-01\n\nBody.\n");
            let entries = parse(&md);
            assert_eq!(entries.len(), 1, "separator {sep:?} should still parse");
            assert_eq!(entries[0].date, "2026-05-01");
        }
    }

    /// A heading with no date is valid; the entry just has an empty date.
    #[test]
    fn heading_without_date_is_accepted() {
        let entries = parse("## [2.0.0]\n\nBody.\n");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].date.is_empty());
    }

    /// Non-semver headings such as `## [Unreleased]` are not versions, so they
    /// must not produce a bogus entry.
    #[test]
    fn ignores_non_semver_headings() {
        let md = "## [1.0.0] — 2026-01-01\n\nReal.\n\n## [Unreleased]\n\nDraft.\n";
        let entries = parse(md);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn empty_and_headingless_input_yields_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("# Title\n\nJust prose, no versions.\n").is_empty());
    }

    /// Out-of-order headings are sorted rather than trusted.
    #[test]
    fn sorts_by_semver_not_file_order() {
        let md = "## [1.0.0] — a\n\nX\n\n## [1.2.0] — b\n\nY\n";
        let entries = parse(md);
        assert_eq!(entries[0].version, Version::parse("1.2.0").unwrap());
    }

    /// Prerelease versions rank below their release, per semver.
    #[test]
    fn prerelease_headings_rank_below_release() {
        let md = "## [1.0.0-dev.1] — a\n\nX\n\n## [1.0.0] — b\n\nY\n";
        let entries = parse(md);
        assert_eq!(entries[0].version, Version::parse("1.0.0").unwrap());
        assert_eq!(entries[1].version, Version::parse("1.0.0-dev.1").unwrap());
    }

    #[test]
    fn rule_detection() {
        assert!(is_rule("---"));
        assert!(is_rule("  ----  "));
        assert!(!is_rule("--"));
        assert!(!is_rule("- item"));
        assert!(!is_rule(""));
    }

    /// Guard against the shipped files drifting out of the shape this parser
    /// assumes: the bundled copies must always yield entries, and the newest
    /// must match the version each component reports.
    #[test]
    fn bundled_files_parse() {
        let game = parse(super::super::BUNDLED_GAME);
        assert!(game.len() > 10, "bundled game changelog should parse");
        let launcher = parse(super::super::BUNDLED_LAUNCHER);
        assert!(launcher.len() > 10, "bundled launcher changelog should parse");
        // The launcher's own crate version must have an entry — if this fails,
        // a release bumped Cargo.toml without writing a changelog section.
        let running = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(
            launcher.iter().any(|e| e.version == running),
            "LauncherChangeLog.md has no entry for the running version {running}"
        );
    }
}
