//! Per-channel game-files release discovery.
//!
//! Lists GitHub Releases for the project, filters by the `game-v` prefix +
//! channel-specific suffix (mirroring `release-client.yml`'s anchored
//! channel-detection regex), parses semver from the tag, returns the highest
//! matching version.
//!
//! Dev gating: callers MUST verify `state.dev_flag == true` before invoking
//! this for `Channel::Dev`. See foundation §3 / Stage 3 plan — unflagged
//! users must not even reach the GitHub API for the dev channel.

use crate::channel::Channel;
use crate::updater::github_client;
use semver::Version;

const REPO_OWNER: &str = "Warstorm548";
const REPO_NAME: &str = "Briska-Blast";
const TAG_PREFIX: &str = "game-v";

/// One game release matching a channel filter.
#[derive(Debug, Clone)]
pub struct GameRelease {
    /// Parsed semver from the tag (includes any `-ea.N` / `-dev.N` prerelease
    /// component so two dev releases at the same base version still compare).
    pub version: Version,
    /// Full tag string, e.g. `game-v0.2.0-dev.1`.
    pub tag: String,
    /// Markdown body of the GitHub Release. May be empty. Stage 4 surfaces
    /// this as the release-notes preview on the update modal.
    #[allow(dead_code)]
    pub body: String,
    /// All assets attached to the release. Caller picks platform-appropriate
    /// one via `installer::select_platform_asset`.
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

/// Fetch the highest-version game release for `channel`. Returns `Ok(None)`
/// when no matching release exists upstream yet (expected during pre-release
/// development before the first `game-v*` tag is pushed).
pub async fn latest_release(channel: Channel) -> Result<Option<GameRelease>, String> {
    // Goes through `github_client` (our owned request) so the rate-limit safety
    // net sees the status + headers; a closed gate or a confirmed `403`/`429`
    // surfaces as the user-facing rate-limit message.
    let releases = github_client::fetch_releases(REPO_OWNER, REPO_NAME)
        .await
        .map_err(|e| e.to_user_string())?;

    let mut best: Option<GameRelease> = None;
    for r in releases {
        let Some(stripped) = r.tag_name.strip_prefix(TAG_PREFIX) else {
            continue;
        };
        let Some(v) = parse_for_channel(stripped, channel) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| v > b.version) {
            best = Some(GameRelease {
                version: v,
                tag: r.tag_name.clone(),
                body: r.body.unwrap_or_default(),
                assets: r
                    .assets
                    .into_iter()
                    // `Asset.url` is the GitHub REST API asset endpoint the
                    // installer downloads from (Accept: octet-stream) — same URL
                    // self_update handed us before.
                    .map(|a| ReleaseAsset {
                        name: a.name,
                        download_url: a.url,
                    })
                    .collect(),
            });
        }
    }
    Ok(best)
}

/// Anchored channel match. The stripped form (no `game-v` prefix) is:
///   stable -> "1.2.3"           (no prerelease at all)
///   ea     -> "1.2.3-ea.4"      (prerelease == "ea.<N>")
///   dev    -> "1.2.3-dev.5"     (prerelease == "dev.<N>")
///
/// Parses through `semver::Version` and inspects the prerelease identifiers
/// directly, so a substring like `-dev.` appearing mid-prerelease (e.g.
/// `1.2.3-pre-dev.1`) doesn't accidentally classify a non-channel build as
/// belonging to dev. The first identifier must equal the channel marker
/// exactly; the second must be a non-empty numeric counter; nothing after.
fn parse_for_channel(stripped: &str, channel: Channel) -> Option<Version> {
    let v = Version::parse(stripped).ok()?;
    match channel {
        Channel::Stable => {
            if v.pre.is_empty() {
                Some(v)
            } else {
                None
            }
        }
        Channel::Ea | Channel::Dev => {
            let marker = if matches!(channel, Channel::Ea) { "ea" } else { "dev" };
            let pre = v.pre.as_str();
            let mut parts = pre.split('.');
            if parts.next()? != marker {
                return None;
            }
            let counter = parts.next()?;
            if counter.is_empty() || !counter.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            // Reject anything beyond exactly `<marker>.<N>` — e.g. a third
            // dotted identifier — so the channel suffix stays anchored.
            if parts.next().is_some() {
                return None;
            }
            Some(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_only_matches_no_suffix() {
        assert!(parse_for_channel("1.2.3", Channel::Stable).is_some());
        assert!(parse_for_channel("1.2.3-dev.1", Channel::Stable).is_none());
        assert!(parse_for_channel("1.2.3-ea.4", Channel::Stable).is_none());
    }

    #[test]
    fn ea_only_matches_ea_suffix() {
        assert!(parse_for_channel("1.2.3-ea.4", Channel::Ea).is_some());
        assert!(parse_for_channel("1.2.3", Channel::Ea).is_none());
        assert!(parse_for_channel("1.2.3-dev.1", Channel::Ea).is_none());
    }

    #[test]
    fn dev_only_matches_dev_suffix() {
        assert!(parse_for_channel("1.2.3-dev.1", Channel::Dev).is_some());
        assert!(parse_for_channel("1.2.3", Channel::Dev).is_none());
        assert!(parse_for_channel("1.2.3-ea.4", Channel::Dev).is_none());
    }

    #[test]
    fn rejects_non_numeric_build_counter() {
        assert!(parse_for_channel("1.2.3-dev.foo", Channel::Dev).is_none());
        assert!(parse_for_channel("1.2.3-dev.1-extra", Channel::Dev).is_none());
    }

    /// Regression: previous substring-split would accept `-dev.` appearing
    /// anywhere in the prerelease (e.g. `pre-dev.1`), mis-classifying
    /// non-channel builds as dev. The semver-aware check rejects them.
    #[test]
    fn rejects_unanchored_channel_marker() {
        assert!(parse_for_channel("1.2.3-pre-dev.1", Channel::Dev).is_none());
        assert!(parse_for_channel("1.2.3-rc-ea.1", Channel::Ea).is_none());
        assert!(parse_for_channel("1.2.3-dev.1.extra", Channel::Dev).is_none());
    }
}
