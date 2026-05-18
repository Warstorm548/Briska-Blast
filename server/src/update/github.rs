use deadpool_redis::redis::AsyncCommands;
use reqwest::Client;
use reqwest::StatusCode;
use semver::Version;
use serde::Deserialize;

const RELEASES_URL: &str =
    "https://api.github.com/repos/Warstorm548/Briska-Blast/releases?per_page=100";

const ETAG_KEY: &str = "update:github_etag";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
}

/// Outcome of a GitHub releases check.
pub enum CheckOutcome {
    /// GitHub returned `304 Not Modified` — caller should preserve cached state.
    NotModified,
    /// A newer matching release was found.
    Update(String),
    /// The check succeeded but no newer release exists.
    NoUpdate,
}

/// Check GitHub for a newer release matching the configured channel.
///
/// - Sends `If-None-Match: <etag>` if a previous ETag is cached in Redis.
/// - Stores the response ETag back into Redis on a 200 response.
/// - Treats `304 Not Modified` as "nothing changed; preserve cached state".
/// - Sends `Authorization: Bearer <GITHUB_TOKEN>` if the env var is set,
///   raising the anonymous-IP rate limit (60/hr) to the authenticated cap.
/// - Requests `per_page=100` and defensively sorts results by semver to
///   avoid relying on GitHub returning releases newest-first.
pub async fn check_for_update(
    client: &Client,
    channel: &str,
    pool: &deadpool_redis::Pool,
) -> Result<CheckOutcome, String> {
    // Read the cached ETag (if any) and send it as If-None-Match.
    let cached_etag: Option<String> = match pool.get().await {
        Ok(mut conn) => conn.get(ETAG_KEY).await.ok(),
        Err(_) => None,
    };

    let mut request = client
        .get(RELEASES_URL)
        .header("User-Agent", "briska-blast-server")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(ref etag) = cached_etag {
        request = request.header("If-None-Match", etag.as_str());
    }
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            request = request.header("Authorization", format!("Bearer {}", token.trim()));
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("github request failed: {e}"))?;

    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(CheckOutcome::NotModified);
    }

    if !response.status().is_success() {
        return Err(format!("github returned {}", response.status()));
    }

    // Capture ETag from the response so we can echo it back next time.
    let new_etag: Option<String> = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let releases: Vec<Release> = response
        .json()
        .await
        .map_err(|e| format!("github response parse failed: {e}"))?;

    // Persist the new ETag (best-effort; missing pool / Redis error is logged
    // but not fatal — we still want to return the parsed update outcome).
    if let Some(ref etag) = new_etag {
        if let Ok(mut conn) = pool.get().await {
            let _: () = conn
                .set(ETAG_KEY, etag.as_str())
                .await
                .inspect_err(|e| tracing::warn!("redis set github_etag failed: {e}"))
                .unwrap_or(());
        }
    }

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("current version unparseable: {e}"))?;

    // Collect every release that matches our channel, parse its semver, and
    // sort descending. This removes the implicit assumption that GitHub
    // returns releases newest-first and tolerates out-of-order hotfix tags.
    let mut candidates: Vec<(Version, String)> = releases
        .into_iter()
        .filter(|r| channel_matches(channel, r))
        .filter_map(|r| {
            let tag = r.tag_name.clone();
            let version_str = tag.trim_start_matches('v');
            Version::parse(version_str).ok().map(|v| (v, tag))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    for (version, tag) in candidates {
        if version > current {
            return Ok(CheckOutcome::Update(tag));
        }
        // First candidate is the maximum; if it's not newer, none will be.
        break;
    }

    Ok(CheckOutcome::NoUpdate)
}

fn channel_matches(channel: &str, release: &Release) -> bool {
    let tag = &release.tag_name;
    match channel {
        "stable" => !release.prerelease && !tag.contains("-ea") && !tag.contains("-dev"),
        "ea"     => release.prerelease && tag.contains("-ea"),
        "dev"    => release.prerelease && tag.contains("-dev"),
        _        => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.to_string(),
            prerelease,
        }
    }

    #[test]
    fn stable_channel_rejects_prereleases() {
        assert!(channel_matches("stable", &rel("v1.2.3", false)));
        assert!(!channel_matches("stable", &rel("v1.2.3-ea.1", true)));
        assert!(!channel_matches("stable", &rel("v1.2.3-dev.1", true)));
        // Even if someone forgot to mark a -ea tag as prerelease on GitHub,
        // the tag-name check still rejects it.
        assert!(!channel_matches("stable", &rel("v1.2.3-ea.1", false)));
    }

    #[test]
    fn ea_channel_accepts_only_ea() {
        assert!(channel_matches("ea", &rel("v1.2.3-ea.1", true)));
        assert!(!channel_matches("ea", &rel("v1.2.3", false)));
        assert!(!channel_matches("ea", &rel("v1.2.3-dev.1", true)));
        // A -ea tag NOT marked prerelease on GitHub is treated as malformed
        // and rejected — prerelease flag is part of the contract.
        assert!(!channel_matches("ea", &rel("v1.2.3-ea.1", false)));
    }

    #[test]
    fn dev_channel_accepts_only_dev() {
        assert!(channel_matches("dev", &rel("v1.2.3-dev.1", true)));
        assert!(!channel_matches("dev", &rel("v1.2.3", false)));
        assert!(!channel_matches("dev", &rel("v1.2.3-ea.1", true)));
    }

    #[test]
    fn unknown_channel_matches_nothing() {
        assert!(!channel_matches("nightly", &rel("v1.2.3", false)));
        assert!(!channel_matches("", &rel("v1.2.3", false)));
    }

    #[test]
    fn semver_prerelease_ordering() {
        // Regression guard: the semver crate must order ea.2 above ea.1 so
        // the descending sort in check_for_update picks the right candidate.
        let v1 = Version::parse("1.2.3-ea.1").unwrap();
        let v2 = Version::parse("1.2.3-ea.2").unwrap();
        let v10 = Version::parse("1.2.3-ea.10").unwrap();
        assert!(v2 > v1);
        assert!(v10 > v2);
        // And a stable release outranks any ea prerelease at the same numeric version.
        let stable = Version::parse("1.2.3").unwrap();
        assert!(stable > v10);
    }
}
