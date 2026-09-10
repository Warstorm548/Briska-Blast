//! Changelog viewer state: the background refresh, the channel filter, the
//! expand/collapse accordion, and opening links in the user's browser.

use crate::app::{AppState, Message};
use crate::changelog::{self, Kind};
use crate::channel::Channel;
use iced::Task;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// How many entries a pane shows. Ten is the agreed window: enough history to
/// be useful, few enough that a user who skipped many versions is not buried.
pub(crate) const WINDOW: usize = 10;

/// Boot tasks: refresh both changelogs and derive the per-channel filter.
///
/// The two refreshes hit `raw.githubusercontent.com`, which is a plain file CDN
/// with no rate-limit budget, so they are safe to fire unconditionally. The
/// filter task reads the shared release snapshot and spends no extra request.
pub(crate) fn boot_tasks() -> Vec<Task<Message>> {
    vec![
        Task::perform(changelog::refresh(Kind::Game), |result| {
            Message::ChangelogRefreshed {
                kind: Kind::Game,
                result,
            }
        }),
        Task::perform(changelog::refresh(Kind::Launcher), |result| {
            Message::ChangelogRefreshed {
                kind: Kind::Launcher,
                result,
            }
        }),
        Task::perform(
            changelog::load_shipped_versions(),
            Message::ChangelogShippedLoaded,
        ),
    ]
}

pub(crate) fn refreshed(
    state: &mut AppState,
    kind: Kind,
    result: Result<Option<Vec<changelog::parse::Entry>>, String>,
) -> Task<Message> {
    match result {
        Ok(Some(entries)) => {
            state.changelog.replace(kind, entries);
            // The visible window just changed, so the seeded "top entry open"
            // may now point at a version that is no longer on top.
            state.changelog_open.remove(&kind);
        }
        Ok(None) => tracing::debug!(?kind, "changelog already current"),
        Err(e) => {
            // Non-fatal by design: the bundled or cached copy stays on screen.
            tracing::warn!(error = %e, ?kind, "changelog refresh failed; keeping local copy");
        }
    }
    Task::none()
}

pub(crate) fn shipped_loaded(
    state: &mut AppState,
    result: Result<BTreeMap<Channel, BTreeSet<Version>>, String>,
) -> Task<Message> {
    match result {
        Ok(map) => state.changelog_shipped = Some(map),
        Err(e) => {
            // Left as `None`, i.e. Pending: without the release list we cannot
            // tell which versions reached this channel, and showing the file
            // unfiltered would put dev-only entries in front of a Stable user.
            tracing::warn!(error = %e, "could not derive per-channel changelog versions");
        }
    }
    Task::none()
}

pub(crate) fn toggled(state: &mut AppState, kind: Kind, version: Version) -> Task<Message> {
    let open = state.changelog_open.entry(kind).or_default();
    if !open.remove(&version) {
        open.insert(version);
    }
    Task::none()
}

/// Open a URL in the user's default browser. `open` is sync, so it runs on the
/// blocking pool rather than stalling the UI thread — mirroring how the
/// Settings folder buttons already call it.
///
/// **Only `http` and `https` are honoured.** Most of what reaches here is a link
/// inside a rendered changelog entry, and that markdown is fetched from the
/// network at runtime — so the URL is data the launcher did not author. Handing
/// an arbitrary scheme to `open::that` delegates to the OS shell, where
/// `file://`, a UNC path, or a registered protocol handler can launch something
/// rather than browse to it. Anything else is dropped with a log line.
pub(crate) fn open_url(url: String) -> Task<Message> {
    if !is_browsable(&url) {
        tracing::warn!(%url, "refusing to open url — only http/https are allowed");
        return Task::none();
    }
    Task::future(async move {
        let target = url.clone();
        let result = tokio::task::spawn_blocking(move || open::that(&target)).await;
        match result {
            Ok(Ok(())) => tracing::info!(%url, "opened url in browser"),
            Ok(Err(e)) => tracing::warn!(error = %e, %url, "failed to open url"),
            Err(e) => tracing::warn!(error = %e, %url, "open task panicked"),
        }
    })
    .discard()
}

/// True only for an absolute `http`/`https` URL. Parsed with the same `url`
/// crate `reqwest` already depends on, so scheme detection is not a string
/// comparison that a crafted input could slip past.
fn is_browsable(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

/// Whether the per-channel changelog filter is known yet.
///
/// The two states must stay distinct. Treating "not loaded" as "no filter" would
/// show the *unfiltered* changelog, and because one file covers every channel
/// that means showing a Stable user entries that only ever shipped to dev. That
/// is not hypothetical: the repo currently has dev-only game releases, so
/// Stable's shipped set is legitimately empty.
pub(crate) enum Filter<'a> {
    /// The release list has not been read yet, or reading it failed. Callers
    /// withhold game entries rather than showing them unfiltered.
    Pending,
    /// Derived. Filter against this set — an **empty** set correctly means "this
    /// channel has no releases yet", and so hides every entry.
    Ready(&'a BTreeSet<Version>),
}

/// The channel filter for `channel`.
pub(crate) fn shipped_for(state: &AppState, channel: Channel) -> Filter<'_> {
    /// A loaded map always carries every channel, but a missing entry is
    /// treated as "no releases" rather than "no filter" — the safe direction.
    static NONE_SHIPPED: std::sync::LazyLock<BTreeSet<Version>> =
        std::sync::LazyLock::new(BTreeSet::new);
    match state.changelog_shipped.as_ref() {
        None => Filter::Pending,
        Some(map) => Filter::Ready(map.get(&channel).unwrap_or(&NONE_SHIPPED)),
    }
}

/// The set of expanded versions for `kind`, seeded so the newest visible entry
/// starts open and the rest start collapsed.
///
/// Seeding lazily at render time (rather than eagerly on load) keeps this
/// correct when the anchored window changes underneath — a channel switch, or a
/// refresh that introduces a newer version.
pub(crate) fn open_set(
    state: &AppState,
    kind: Kind,
    visible: &[&changelog::Section],
) -> HashSet<Version> {
    match state.changelog_open.get(&kind) {
        Some(open) => open.clone(),
        None => visible
            .first()
            .map(|s| HashSet::from([s.version().clone()]))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changelog::parse::Entry;
    use crate::changelog::Section;

    fn entry(v: &str) -> Entry {
        Entry {
            version: Version::parse(v).unwrap(),
            date: "2026-01-01".into(),
            body: "body".into(),
        }
    }

    fn section(v: &str) -> Section {
        Section::new(entry(v))
    }

    #[test]
    fn toggle_opens_then_closes() {
        let mut state = AppState::default();
        let v = Version::parse("1.0.0").unwrap();

        let _ = toggled(&mut state, Kind::Game, v.clone());
        assert!(state.changelog_open[&Kind::Game].contains(&v));

        let _ = toggled(&mut state, Kind::Game, v.clone());
        assert!(!state.changelog_open[&Kind::Game].contains(&v));
    }

    /// The two changelogs keep independent open sets.
    #[test]
    fn toggle_is_per_changelog() {
        let mut state = AppState::default();
        let v = Version::parse("1.0.0").unwrap();
        let _ = toggled(&mut state, Kind::Game, v.clone());
        assert!(!state
            .changelog_open
            .get(&Kind::Launcher)
            .is_some_and(|s| s.contains(&v)));
    }

    #[test]
    fn open_set_seeds_the_newest_entry() {
        let state = AppState::default();
        let entries = [section("1.2.0"), section("1.1.0")];
        let visible: Vec<&Section> = entries.iter().collect();
        let open = open_set(&state, Kind::Game, &visible);
        assert_eq!(open.len(), 1);
        assert!(open.contains(&Version::parse("1.2.0").unwrap()));
    }

    /// Once the user has interacted, their set wins — including a set that
    /// collapses everything.
    #[test]
    fn open_set_respects_an_explicit_empty_set() {
        let mut state = AppState::default();
        state.changelog_open.insert(Kind::Game, HashSet::new());
        let entries = [section("1.2.0")];
        let visible: Vec<&Section> = entries.iter().collect();
        assert!(open_set(&state, Kind::Game, &visible).is_empty());
    }

    #[test]
    fn open_set_on_an_empty_window_is_empty() {
        let state = AppState::default();
        assert!(open_set(&state, Kind::Game, &[]).is_empty());
    }

    /// "Not loaded" and "loaded but this channel has shipped nothing" are
    /// different answers. Collapsing them would drop the filter and show the
    /// unfiltered file — which, with the repo's dev-only game releases, means
    /// showing a Stable user entries that never reached Stable.
    #[test]
    fn pending_and_empty_shipped_sets_are_distinct() {
        let mut state = AppState::default();
        // Nothing loaded yet.
        assert!(matches!(
            shipped_for(&state, Channel::Stable),
            Filter::Pending
        ));

        // Loaded, and this channel genuinely has no releases: a real filter
        // that happens to match nothing, NOT an absent filter.
        state.changelog_shipped = Some(BTreeMap::from([(Channel::Stable, BTreeSet::new())]));
        let Filter::Ready(set) = shipped_for(&state, Channel::Stable) else {
            panic!("a loaded empty set must be Ready, not Pending");
        };
        assert!(set.is_empty());

        // Loaded with content.
        state.changelog_shipped = Some(BTreeMap::from([(
            Channel::Stable,
            BTreeSet::from([Version::parse("1.0.0").unwrap()]),
        )]));
        let Filter::Ready(set) = shipped_for(&state, Channel::Stable) else {
            panic!("a loaded non-empty set must be Ready");
        };
        assert_eq!(set.len(), 1);
    }

    /// A loaded map always carries every channel, but a missing entry must fall
    /// to "nothing shipped" rather than "no filter" — the safe direction.
    #[test]
    fn missing_channel_in_a_loaded_map_filters_everything() {
        let state = AppState {
            changelog_shipped: Some(BTreeMap::new()),
            ..Default::default()
        };
        let Filter::Ready(set) = shipped_for(&state, Channel::Dev) else {
            panic!("a loaded map must be Ready even when the channel is absent");
        };
        assert!(set.is_empty());
    }

    /// Changelog markdown is fetched at runtime, so a link inside an entry is
    /// untrusted input. Only real http/https URLs may reach the OS shell.
    #[test]
    fn only_http_and_https_urls_are_browsable() {
        assert!(is_browsable("https://github.com/Warstorm548/Briska-Blast"));
        assert!(is_browsable("http://example.test/notes"));

        // Schemes that would make `open::that` launch rather than browse.
        assert!(!is_browsable("file:///etc/passwd"));
        assert!(!is_browsable("file://server/share/payload.exe"));
        assert!(!is_browsable("javascript:alert(1)"));
        assert!(!is_browsable("ms-msdt:/id"));
        assert!(!is_browsable("data:text/html,<script>"));
        // Not absolute URLs at all.
        assert!(!is_browsable("\\\\server\\share"));
        assert!(!is_browsable("/usr/bin/sh"));
        assert!(!is_browsable("not a url"));
        assert!(!is_browsable(""));
        // Scheme matching is exact, not prefix-based.
        assert!(!is_browsable("httpsx://example.test"));
    }

    /// A failed derivation must stay Pending, so callers withhold entries
    /// instead of falling back to the unfiltered changelog.
    #[test]
    fn failed_shipped_load_stays_pending() {
        let mut state = AppState::default();
        let _ = shipped_loaded(&mut state, Err("offline".into()));
        assert!(matches!(
            shipped_for(&state, Channel::Stable),
            Filter::Pending
        ));
    }

    /// A refresh that lands a newer version must drop the seeded open set, so
    /// the accordion re-seeds onto the new top entry instead of leaving a
    /// mid-list entry expanded.
    #[test]
    fn refresh_resets_the_seeded_open_set() {
        let mut state = AppState::default();
        let _ = toggled(&mut state, Kind::Game, Version::parse("1.0.0").unwrap());
        assert!(state.changelog_open.contains_key(&Kind::Game));

        let _ = refreshed(&mut state, Kind::Game, Ok(Some(vec![entry("2.0.0")])));
        assert!(!state.changelog_open.contains_key(&Kind::Game));
    }

    /// A failed refresh must leave the loaded copy (and the user's open set)
    /// alone — the bundled changelog is still perfectly readable.
    #[test]
    fn failed_refresh_keeps_local_copy() {
        let mut state = AppState::default();
        let before = state.changelog.entries(Kind::Game).len();
        let _ = refreshed(&mut state, Kind::Game, Err("offline".into()));
        assert_eq!(state.changelog.entries(Kind::Game).len(), before);
    }
}
