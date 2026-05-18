use reqwest::Client;
use semver::Version;
use serde::Deserialize;

const RELEASES_URL: &str =
    "https://api.github.com/repos/Warstorm548/Briska-Blast/releases";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
}

/// Check GitHub for a newer release matching the configured channel.
///
/// Returns:
/// - `Ok(Some(tag))` — a newer matching version was found
/// - `Ok(None)` — no newer release exists (truly up to date)
/// - `Err(msg)` — the check itself failed (network/parse error); caller must not
///   treat this as "up to date" and must not clear cached state
pub async fn check_for_update(client: &Client, channel: &str) -> Result<Option<String>, String> {
    let response = client
        .get(RELEASES_URL)
        .header("User-Agent", "briska-blast-server")
        .send()
        .await
        .map_err(|e| format!("github request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("github returned {}", response.status()));
    }

    let releases: Vec<Release> = response
        .json()
        .await
        .map_err(|e| format!("github response parse failed: {e}"))?;

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("current version unparseable: {e}"))?;

    for release in releases {
        let tag = &release.tag_name;
        let matches = match channel {
            "stable" => !release.prerelease && !tag.contains("-ea") && !tag.contains("-dev"),
            "ea"     => release.prerelease && tag.contains("-ea"),
            "dev"    => release.prerelease && tag.contains("-dev"),
            _        => false,
        };

        if !matches {
            continue;
        }

        // Parse the full semver including prerelease identifiers so the semver
        // crate can correctly compare "v1.2.3-ea.2" vs "v1.2.3-ea.1".
        let version_str = tag.trim_start_matches('v');
        if let Ok(latest) = Version::parse(version_str) {
            if latest > current {
                return Ok(Some(tag.clone()));
            }
        }
    }

    Ok(None)
}
