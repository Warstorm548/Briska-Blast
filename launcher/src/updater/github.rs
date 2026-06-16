//! GitHub-Releases-backed self-update via the `self_update` crate.
//!
//! Discovery: list all releases on `Warstorm548/Briska-Blast`, keep ones whose
//! tag begins with `launcher-v`, parse the suffix as semver, pick the highest
//! one greater than the running version.
//!
//! Application: `self_update`'s rename-trick — rename the running binary,
//! drop the downloaded binary in its place, return. Caller must
//! `std::process::exit(0)` immediately after; the renamed orphan is cleaned up
//! by the next launcher run via `self_update`'s own logic.

use super::github_client;
use semver::Version;

const REPO_OWNER: &str = "Warstorm548";
const REPO_NAME: &str = "Briska-Blast";
const TAG_PREFIX: &str = "launcher-v";
const BIN_NAME: &str = "briskablast-launcher";

/// Result of `check_for_update`.
#[derive(Debug, Clone)]
pub enum UpdateCheckOutcome {
    UpToDate,
    Available {
        /// Parsed semver string, e.g. `"0.4.0-dev.1"`.
        version: String,
        /// Markdown body of the GitHub Release. May be empty.
        notes: String,
    },
}

/// Query GitHub Releases for a newer `launcher-v*` than the running binary.
/// Suitable for `iced::Task::perform`. Goes through `github_client` (our owned
/// request) so the rate-limit safety net can see the status + headers; a closed
/// gate or a confirmed `403`/`429` surfaces as the user-facing rate-limit message.
pub async fn check_for_update() -> Result<UpdateCheckOutcome, String> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("invalid current version {:?}: {e}", env!("CARGO_PKG_VERSION")))?;
    tracing::debug!(%current, "querying GitHub Releases for launcher updates");

    let releases = github_client::fetch_releases(REPO_OWNER, REPO_NAME)
        .await
        .map_err(|e| e.to_user_string())?;

    let mut best: Option<(Version, &github_client::Release)> = None;
    for r in &releases {
        // `tag_name` is the git tag string.
        let Some(stripped) = r.tag_name.strip_prefix(TAG_PREFIX) else {
            continue;
        };
        let Ok(v) = Version::parse(stripped) else {
            tracing::trace!(tag = %r.tag_name, "skipping unparseable launcher tag");
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            best = Some((v, r));
        }
    }

    let Some((latest, release)) = best else {
        tracing::info!("no launcher-v* releases found upstream — treating as up to date");
        return Ok(UpdateCheckOutcome::UpToDate);
    };
    if latest <= current {
        tracing::info!(%latest, %current, "launcher is up to date");
        return Ok(UpdateCheckOutcome::UpToDate);
    }

    tracing::info!(%latest, %current, "launcher update available");
    Ok(UpdateCheckOutcome::Available {
        version: latest.to_string(),
        notes: release.body.clone().unwrap_or_default(),
    })
}

/// Async wrapper for the binary-swap. Returns Ok(()) on a successful swap;
/// caller MUST then `std::process::exit(0)` because the running binary has
/// been renamed and replaced on disk.
pub async fn run_self_update(version: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || run_self_update_blocking(&version))
        .await
        .map_err(|e| format!("self-update join error: {e}"))?
}

fn run_self_update_blocking(version: &str) -> Result<(), String> {
    let tag = format!("{TAG_PREFIX}{version}");
    tracing::info!(target = %tag, "starting self-update binary swap");

    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(env!("CARGO_PKG_VERSION"))
        .target_version_tag(&tag)
        // GUI process — keep self_update from drawing its own indicatif bar.
        .show_download_progress(false)
        .show_output(false)
        .no_confirm(true)
        .build()
        .map_err(|e| format!("update build: {e}"))?
        .update()
        .map_err(|e| format!("update run: {e}"))?;

    tracing::info!(?status, "self-update finished");
    Ok(())
}
