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
    tokio::task::spawn_blocking(move || latest_release_blocking(channel))
        .await
        .map_err(|e| format!("game release fetch join: {e}"))?
}

fn latest_release_blocking(channel: Channel) -> Result<Option<GameRelease>, String> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("release list build: {e}"))?
        .fetch()
        .map_err(|e| format!("release list fetch: {e}"))?;

    let mut best: Option<GameRelease> = None;
    for r in releases {
        let Some(stripped) = r.version.strip_prefix(TAG_PREFIX) else {
            continue;
        };
        let Some(v) = parse_for_channel(stripped, channel) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| v > b.version) {
            best = Some(GameRelease {
                version: v,
                tag: r.version.clone(),
                body: r.body.unwrap_or_default(),
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| ReleaseAsset {
                        name: a.name,
                        download_url: a.download_url,
                    })
                    .collect(),
            });
        }
    }
    Ok(best)
}

/// Anchored channel match. The stripped form (no `game-v` prefix) is:
///   stable -> "1.2.3"           (no dash)
///   ea     -> "1.2.3-ea.4"
///   dev    -> "1.2.3-dev.5"
/// The build counter N must be purely numeric — rejects `1.2.3-dev.1-foo`.
fn parse_for_channel(stripped: &str, channel: Channel) -> Option<Version> {
    let suffix = match channel {
        Channel::Stable => {
            if stripped.contains('-') {
                return None;
            }
            return Version::parse(stripped).ok();
        }
        Channel::Ea => "-ea.",
        Channel::Dev => "-dev.",
    };
    let (_base, after) = stripped.split_once(suffix)?;
    if after.is_empty() || !after.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Version::parse(stripped).ok()
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
}
