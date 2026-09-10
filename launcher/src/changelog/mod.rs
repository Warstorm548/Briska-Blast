//! In-app changelogs for the game and the launcher.
//!
//! Three sources, in precedence order:
//!
//! 1. **Runtime refresh** from `raw.githubusercontent.com` at the `dev` ref, so
//!    a running launcher can describe versions published after it was built.
//! 2. **Disk cache** of that refresh under `<data_dir>/changelogs/`, so the last
//!    known text survives being offline.
//! 3. **Bundled copy** compiled in with `include_str!`, so a first run with no
//!    network still has history.
//!
//! Raw file fetches go to a **separate CDN** from the REST API: they carry no
//! `x-ratelimit-*` headers and do not spend the 60/hour core budget, so unlike
//! `updater::release_cache` this module is deliberately *not* behind
//! `crate::ratelimit`'s gate. They do support `ETag`, so a repeat fetch of an
//! unchanged file is a `304` with no body.
//!
//! Reading the `dev` ref (rather than the default branch) is deliberate: it is
//! the branch that gets committed to first. Because the file is shared across
//! channels, [`shipped_versions`] filters entries down to the versions actually
//! released on a given channel, so a Stable user never reads about a version
//! that only ever existed on dev.

pub mod fetch;
pub mod parse;

use crate::channel::Channel;
use crate::paths;
use crate::updater::Release;
use parse::Entry;
use semver::Version;
use std::collections::BTreeSet;

/// Compiled-in baseline. Bundling the whole file rather than a trimmed window
/// costs ~182 KB across both and needs no build script, and it gives the
/// offline fallback more history than the UI's 10-entry window — which matters
/// for a user who has skipped many versions.
pub(crate) const BUNDLED_GAME: &str = include_str!("../../../GameChangeLog.md");
pub(crate) const BUNDLED_LAUNCHER: &str = include_str!("../../../LauncherChangeLog.md");

const REPO_OWNER: &str = "Warstorm548";
const REPO_NAME: &str = "Briska-Blast";
/// Branch the changelogs are read from. See the module docs.
const REF: &str = "dev";

/// Which changelog. The game's is channel-filtered; the launcher's is not,
/// because there is one launcher rather than one per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Game,
    Launcher,
}

impl Kind {
    pub fn file_name(self) -> &'static str {
        match self {
            Kind::Game => "GameChangeLog.md",
            Kind::Launcher => "LauncherChangeLog.md",
        }
    }

    /// Plain-file URL used for the runtime refresh. Not the REST API.
    fn raw_url(self) -> String {
        format!(
            "https://raw.githubusercontent.com/{REPO_OWNER}/{REPO_NAME}/{REF}/{}",
            self.file_name()
        )
    }

    /// Human-facing URL behind "View the full changelog on GitHub".
    pub fn web_url(self) -> String {
        format!(
            "https://github.com/{REPO_OWNER}/{REPO_NAME}/blob/{REF}/{}",
            self.file_name()
        )
    }

    fn bundled(self) -> &'static str {
        match self {
            Kind::Game => BUNDLED_GAME,
            Kind::Launcher => BUNDLED_LAUNCHER,
        }
    }
}

/// One entry together with its pre-parsed markdown.
///
/// `markdown::view` borrows its items, so they cannot be produced inside a view
/// function and returned — they have to be owned by something that outlives the
/// borrow. Parsing once at load time (rather than per frame) also keeps
/// scrolling cheap.
#[derive(Debug)]
pub struct Section {
    pub entry: Entry,
    pub md: Vec<iced::widget::markdown::Item>,
}

impl Section {
    pub fn new(entry: Entry) -> Self {
        let md = iced::widget::markdown::parse(&entry.body).collect();
        Self { entry, md }
    }

    pub fn version(&self) -> &Version {
        &self.entry.version
    }
}

/// Parsed changelogs held in `AppState`.
#[derive(Debug, Default)]
pub struct Store {
    game: Vec<Section>,
    launcher: Vec<Section>,
}

impl Store {
    /// Build from the best locally available source for each kind — no network.
    /// Called on boot; the refresh lands later via [`refresh`].
    pub fn load() -> Self {
        Self {
            game: sections(load_best(Kind::Game)),
            launcher: sections(load_best(Kind::Launcher)),
        }
    }

    pub fn entries(&self, kind: Kind) -> &[Section] {
        match kind {
            Kind::Game => &self.game,
            Kind::Launcher => &self.launcher,
        }
    }

    /// Swap in a newly refreshed parse.
    pub fn replace(&mut self, kind: Kind, entries: Vec<Entry>) {
        let built = sections(entries);
        match kind {
            Kind::Game => self.game = built,
            Kind::Launcher => self.launcher = built,
        }
    }
}

fn sections(entries: Vec<Entry>) -> Vec<Section> {
    entries.into_iter().map(Section::new).collect()
}

/// Pick the better of the disk cache and the bundled copy.
///
/// Comparing top versions rather than always preferring one source handles the
/// case that actually bites: a launcher that just self-updated ships a *newer*
/// bundled changelog than whatever its disk cache last recorded.
fn load_best(kind: Kind) -> Vec<Entry> {
    let bundled = parse::parse(kind.bundled());
    let Some(text) = read_cached(kind) else {
        return bundled;
    };
    let cached = parse::parse(&text);
    match (cached.first(), bundled.first()) {
        (Some(c), Some(b)) if c.version >= b.version => cached,
        (Some(_), None) => cached,
        _ => bundled,
    }
}

fn read_cached(kind: Kind) -> Option<String> {
    let path = paths::changelog_dir().ok()?.join(kind.file_name());
    std::fs::read_to_string(path).ok()
}

/// Refresh one changelog from GitHub.
///
/// `Ok(Some(entries))` = the file changed and has been re-parsed and persisted.
/// `Ok(None)` = unchanged (`304`), keep what is loaded. `Err` = the fetch
/// failed; callers keep what is loaded and carry on, since a stale changelog is
/// a cosmetic problem rather than a broken launcher.
pub async fn refresh(kind: Kind) -> Result<Option<Vec<Entry>>, String> {
    let etag = fetch::read_etag(kind);
    match fetch::get(&kind.raw_url(), etag.as_deref()).await {
        Ok(fetch::Outcome::NotModified) => {
            tracing::debug!(file = kind.file_name(), "changelog unchanged (304)");
            Ok(None)
        }
        Ok(fetch::Outcome::Fresh { text, etag }) => {
            let entries = parse::parse(&text);
            if entries.is_empty() {
                // Refuse to overwrite a good cache with something unparseable —
                // most likely an error page or a truncated body.
                return Err(format!(
                    "{} fetched but yielded no entries; keeping the previous copy",
                    kind.file_name()
                ));
            }
            fetch::persist(kind, &text, etag.as_deref());
            tracing::info!(
                file = kind.file_name(),
                entries = entries.len(),
                "changelog refreshed from GitHub"
            );
            Ok(Some(entries))
        }
        Err(e) => Err(e),
    }
}

// ---- selection (pure, unit-tested) ----

/// Strip any prerelease/build metadata: changelog headings are plain
/// `MAJOR.MINOR.PATCH`, while an installed version carries the channel suffix
/// (`0.35.1-dev.1`). Comparing bases is what lets the two meet.
pub fn base(v: &Version) -> Version {
    Version::new(v.major, v.minor, v.patch)
}

/// Base versions of every `game-v*` release published on `channel`.
///
/// This is the channel filter: the changelog file is shared across channels and
/// its headings carry no channel marker, but the release tags do. Derived from
/// the list `updater::release_cache` already holds, so it costs no request.
pub fn shipped_versions(releases: &[Release], channel: Channel) -> BTreeSet<Version> {
    releases
        .iter()
        .filter_map(|r| r.tag_name.strip_prefix(crate::updater::branches::GAME_TAG_PREFIX))
        .filter_map(|stripped| crate::updater::branches::parse_for_channel(stripped, channel))
        .map(|v| base(&v))
        .collect()
}

/// Per-channel shipped-version sets for all three channels, read off the shared
/// release snapshot. Suitable for `iced::Task::perform`; costs no extra request
/// because `Freshness::Cached` reuses whatever the boot fan-out already fetched.
pub async fn load_shipped_versions(
) -> Result<std::collections::BTreeMap<Channel, BTreeSet<Version>>, String> {
    use crate::updater::release_cache::Freshness;
    let releases = crate::updater::branches::all_releases(Freshness::Cached).await?;
    Ok(Channel::all()
        .iter()
        .map(|&c| (c, shipped_versions(&releases, c)))
        .collect())
}

/// Entries at or below `anchor`, newest first, capped at `limit`.
///
/// This is the channel pane: the version the user actually has on disk sits at
/// the top and everything above it is hidden, so the pane reads as "what you
/// have" rather than "what exists".
pub fn anchored<'a>(
    entries: &'a [Section],
    shipped: Option<&BTreeSet<Version>>,
    anchor: &Version,
    limit: usize,
) -> Vec<&'a Section> {
    let anchor = base(anchor);
    entries
        .iter()
        .filter(|s| *s.version() <= anchor)
        .filter(|s| shipped.is_none_or(|set| set.contains(s.version())))
        .take(limit)
        .collect()
}

/// Entries strictly newer than `from` and at or below `to`, newest first,
/// capped at `limit`.
///
/// This is the update prompt: everything the user is about to receive. `from`
/// is `None` for a fresh install, which reduces to just the target's entry.
pub fn range<'a>(
    entries: &'a [Section],
    shipped: Option<&BTreeSet<Version>>,
    from: Option<&Version>,
    to: &Version,
    limit: usize,
) -> Vec<&'a Section> {
    let to = base(to);
    let from = from.map(base);
    entries
        .iter()
        .filter(|s| *s.version() <= to)
        .filter(|s| match from.as_ref() {
            Some(f) => s.version() > f,
            // Fresh install: only the version being installed.
            None => *s.version() == to,
        })
        .filter(|s| shipped.is_none_or(|set| set.contains(s.version())))
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(v: &str) -> Section {
        Section::new(Entry {
            version: Version::parse(v).unwrap(),
            date: "2026-01-01".into(),
            body: format!("body {v}"),
        })
    }

    fn sample() -> Vec<Section> {
        ["0.35.1", "0.35.0", "0.34.1", "0.34.0", "0.33.0"]
            .iter()
            .map(|v| entry(v))
            .collect()
    }

    fn shipped(vs: &[&str]) -> BTreeSet<Version> {
        vs.iter().map(|v| Version::parse(v).unwrap()).collect()
    }

    #[test]
    fn base_strips_the_channel_suffix() {
        let dev = Version::parse("0.35.1-dev.4").unwrap();
        assert_eq!(base(&dev), Version::parse("0.35.1").unwrap());
        // A plain release is unchanged.
        let plain = Version::parse("0.35.1").unwrap();
        assert_eq!(base(&plain), plain);
    }

    #[test]
    fn anchored_starts_at_the_installed_version() {
        let entries = sample();
        let got = anchored(&entries, None, &Version::parse("0.34.1").unwrap(), 10);
        let versions: Vec<_> = got.iter().map(|s| s.version().to_string()).collect();
        assert_eq!(versions, ["0.34.1", "0.34.0", "0.33.0"]);
    }

    /// A dev install like `0.35.1-dev.1` must anchor at the `0.35.1` heading,
    /// not fall through to the entry below it.
    #[test]
    fn anchored_matches_a_prerelease_install() {
        let entries = sample();
        let got = anchored(&entries, None, &Version::parse("0.35.1-dev.1").unwrap(), 10);
        assert_eq!(*got[0].version(), Version::parse("0.35.1").unwrap());
    }

    #[test]
    fn anchored_respects_the_limit() {
        let entries = sample();
        let got = anchored(&entries, None, &Version::parse("0.35.1").unwrap(), 2);
        assert_eq!(got.len(), 2);
    }

    /// The channel filter drops versions that never shipped to this channel,
    /// even when they sit inside the anchored window.
    #[test]
    fn anchored_applies_the_channel_filter() {
        let entries = sample();
        let s = shipped(&["0.35.1", "0.34.0", "0.33.0"]);
        let got = anchored(&entries, Some(&s), &Version::parse("0.35.1").unwrap(), 10);
        let versions: Vec<_> = got.iter().map(|s| s.version().to_string()).collect();
        assert_eq!(versions, ["0.35.1", "0.34.0", "0.33.0"]);
    }

    #[test]
    fn range_covers_everything_being_installed() {
        let entries = sample();
        let got = range(
            &entries,
            None,
            Some(&Version::parse("0.33.0").unwrap()),
            &Version::parse("0.35.0").unwrap(),
            10,
        );
        let versions: Vec<_> = got.iter().map(|s| s.version().to_string()).collect();
        assert_eq!(versions, ["0.35.0", "0.34.1", "0.34.0"]);
    }

    /// The user's current version is not part of what they are about to get.
    #[test]
    fn range_excludes_the_installed_version() {
        let entries = sample();
        let got = range(
            &entries,
            None,
            Some(&Version::parse("0.35.0").unwrap()),
            &Version::parse("0.35.1").unwrap(),
            10,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(*got[0].version(), Version::parse("0.35.1").unwrap());
    }

    #[test]
    fn range_on_a_fresh_install_shows_only_the_target() {
        let entries = sample();
        let got = range(&entries, None, None, &Version::parse("0.34.1").unwrap(), 10);
        assert_eq!(got.len(), 1);
        assert_eq!(*got[0].version(), Version::parse("0.34.1").unwrap());
    }

    /// A user who skipped many versions gets the newest `limit`, not all of
    /// them — the cap keeps the prompt readable.
    #[test]
    fn range_caps_a_long_skip() {
        let entries = sample();
        let got = range(
            &entries,
            None,
            Some(&Version::parse("0.33.0").unwrap()),
            &Version::parse("0.35.1").unwrap(),
            2,
        );
        let versions: Vec<_> = got.iter().map(|s| s.version().to_string()).collect();
        assert_eq!(versions, ["0.35.1", "0.35.0"]);
    }

    #[test]
    fn range_prerelease_bounds_compare_by_base() {
        let entries = sample();
        let got = range(
            &entries,
            None,
            Some(&Version::parse("0.34.1-dev.2").unwrap()),
            &Version::parse("0.35.1-dev.1").unwrap(),
            10,
        );
        let versions: Vec<_> = got.iter().map(|s| s.version().to_string()).collect();
        assert_eq!(versions, ["0.35.1", "0.35.0"]);
    }

    #[test]
    fn shipped_versions_splits_by_channel() {
        let releases = vec![
            release("game-v0.35.1-dev.1"),
            release("game-v0.35.0"),
            release("game-v0.34.1-ea.2"),
            release("launcher-v0.20.1"),
            release("v0.36.1-dev.1"),
        ];
        let dev = shipped_versions(&releases, Channel::Dev);
        assert!(dev.contains(&Version::parse("0.35.1").unwrap()));
        assert_eq!(dev.len(), 1, "only the game-v dev tag counts");

        let stable = shipped_versions(&releases, Channel::Stable);
        assert!(stable.contains(&Version::parse("0.35.0").unwrap()));
        assert_eq!(stable.len(), 1);

        let ea = shipped_versions(&releases, Channel::Ea);
        assert!(ea.contains(&Version::parse("0.34.1").unwrap()));
        assert_eq!(ea.len(), 1);
    }

    fn release(tag: &str) -> Release {
        // serde is the only constructor — Release has no public builder.
        serde_json::from_value(serde_json::json!({
            "tag_name": tag,
            "body": null,
            "assets": [],
        }))
        .unwrap()
    }
}
