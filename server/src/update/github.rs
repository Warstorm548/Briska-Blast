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

pub async fn check_for_update(client: &Client, channel: &str) -> Option<String> {
    let releases: Vec<Release> = client
        .get(RELEASES_URL)
        .header("User-Agent", "briska-blast-server")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;

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

        let version_str = tag.trim_start_matches('v');
        let version_str = if let Some(idx) = version_str.find('-') {
            &version_str[..idx]
        } else {
            version_str
        };

        if let Ok(latest) = Version::parse(version_str) {
            if latest > current {
                return Some(tag.clone());
            }
        }
    }

    None
}
