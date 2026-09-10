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
        Ok(map) => state.changelog_shipped = map,
        Err(e) => {
            // Leaving the map empty means "no filter yet" rather than "nothing
            // shipped", so the pane still renders — see `shipped_for`.
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
pub(crate) fn open_url(url: String) -> Task<Message> {
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

/// The channel filter for `channel`, or `None` when it is not known yet.
///
/// An absent or empty set means "not derived yet" and must read as *no filter*.
/// Treating it as "nothing shipped" would blank the pane on every boot until
/// the release list lands, which is the wrong failure direction for a viewer.
pub(crate) fn shipped_for(state: &AppState, channel: Channel) -> Option<&BTreeSet<Version>> {
    state
        .changelog_shipped
        .get(&channel)
        .filter(|s| !s.is_empty())
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

    /// An undelivered or empty filter must read as "no filter", never as
    /// "nothing shipped" — otherwise the pane blanks on every boot.
    #[test]
    fn missing_or_empty_filter_reads_as_no_filter() {
        let mut state = AppState::default();
        assert!(shipped_for(&state, Channel::Stable).is_none());

        state
            .changelog_shipped
            .insert(Channel::Stable, BTreeSet::new());
        assert!(shipped_for(&state, Channel::Stable).is_none());

        state.changelog_shipped.insert(
            Channel::Stable,
            BTreeSet::from([Version::parse("1.0.0").unwrap()]),
        );
        assert!(shipped_for(&state, Channel::Stable).is_some());
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
