//! GitHub-Releases-backed self-update via the `self_update` crate.
//!
//! Discovery: list all releases on `Warstorm548/Briska-Blast`, keep ones whose
//! tag begins with `launcher-v`, parse the suffix as semver, pick the highest
//! one greater than the running version.
//!
//! Application is per-OS (dispatched in [`run_self_update`]):
//! * **Windows** — `self_update`'s rename-trick: rename the running exe, drop
//!   the downloaded one in its place.
//! * **macOS** — whole-bundle swap ([`super::macos_bundle`]) when running from
//!   a `.app`; the rename-trick only for a bare binary.
//! * **Linux** — outer-file replacement ([`super::appimage`]) when running as
//!   an AppImage; the rename-trick for a user-writable bare binary; a
//!   "download the new .deb" message for system-wide (`/usr/…`) installs.
//!
//! In every case the caller must `std::process::exit(0)` immediately after a
//! successful swap; leftovers are cleaned up on the next launcher run
//! (`self_update`'s own logic on Windows, `cleanup.rs` elsewhere).

use super::github_client;
use semver::Version;
use std::path::Path;

pub(super) const REPO_OWNER: &str = "Warstorm548";
pub(super) const REPO_NAME: &str = "Briska-Blast";
pub(super) const TAG_PREFIX: &str = "launcher-v";
const BIN_NAME: &str = "briskablast-launcher";

/// User-facing refusal for installs the unelevated swap can never write to
/// (the .deb lands the binary in root-owned `/usr/bin`).
#[cfg(target_os = "linux")]
const SYSTEM_INSTALL_MSG: &str = "This launcher is installed system-wide (e.g. via the \
    .deb package), which self-update cannot overwrite. Download the new .deb from the \
    GitHub Releases page to update.";

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

/// Apply the update for `version` using the platform-appropriate swap (see
/// module docs). Returns Ok(()) on a successful swap; caller MUST then
/// `std::process::exit(0)` because the launcher on disk has been replaced.
pub async fn run_self_update(version: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Installed the normal way (.dmg → .app), the running exe is at
        // <bundle>.app/Contents/MacOS/…: replace the whole bundle so the
        // ad-hoc signature seal stays valid. A bare binary (no bundle)
        // falls through to the self_update rename-trick below.
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        if let Some(bundle) = bundle_root(&exe) {
            return super::macos_bundle::update_bundle(&version, &bundle).await;
        }
    }
    #[cfg(target_os = "linux")]
    {
        // AppImage: the exe lives in a read-only squashfs mount, so the
        // outer .AppImage file (env set by the AppImage runtime) is
        // replaced instead of the running binary.
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let path = std::path::PathBuf::from(appimage);
            return super::appimage::update_appimage(&version, &path).await;
        }
        // System-wide (.deb) install: refuse up front with a useful message
        // instead of letting the swap die on permission-denied.
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        if is_system_installed(&exe) {
            tracing::warn!(exe = %exe.display(), "refusing self-update of system-wide install");
            return Err(SYSTEM_INSTALL_MSG.into());
        }
    }

    let result = tokio::task::spawn_blocking(move || run_self_update_blocking(&version))
        .await
        .map_err(|e| format!("self-update join error: {e}"))?;
    // Safety net for writable-looking installs that still aren't (e.g. a
    // root-owned copy outside /usr): map the raw permission error to the
    // same actionable message as the up-front check.
    #[cfg(target_os = "linux")]
    let result = result.map_err(|e| {
        if e.contains("os error 13") || e.to_lowercase().contains("permission denied") {
            format!("{SYSTEM_INSTALL_MSG} ({e})")
        } else {
            e
        }
    });
    result
}

/// Resolve the `.app` bundle root from an executable path of the canonical
/// `<root>.app/Contents/MacOS/<exe>` shape. `None` for a bare binary. Pure
/// path logic (compiled everywhere so the tests run on any host); used by the
/// macOS dispatch above and the bundle-leftover cleanup.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn bundle_root(exe: &Path) -> Option<std::path::PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension()? != "app" {
        return None;
    }
    Some(bundle.to_path_buf())
}

/// True when the exe lives under the root-owned system prefix (`cargo deb`
/// installs to `/usr/bin`) — the unelevated swap can only fail there.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_system_installed(exe: &Path) -> bool {
    exe.starts_with("/usr")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_root_resolves_canonical_app_layout() {
        let exe = Path::new(
            "/Applications/BriskaBlast Launcher.app/Contents/MacOS/briskablast-launcher",
        );
        assert_eq!(
            bundle_root(exe),
            Some("/Applications/BriskaBlast Launcher.app".into())
        );
        // Nested install location (e.g. ~/Applications) works the same.
        let nested = Path::new(
            "/Users/x/Applications/Foo.app/Contents/MacOS/briskablast-launcher",
        );
        assert_eq!(bundle_root(nested), Some("/Users/x/Applications/Foo.app".into()));
    }

    #[test]
    fn bundle_root_rejects_non_bundle_shapes() {
        // Bare binary.
        assert_eq!(bundle_root(Path::new("/usr/local/bin/briskablast-launcher")), None);
        // Right depth, wrong directory names.
        assert_eq!(
            bundle_root(Path::new("/Applications/Foo.app/Contents/Resources/bin")),
            None
        );
        assert_eq!(bundle_root(Path::new("/a/Foo.dir/Contents/MacOS/bin")), None);
        // Missing the .app ancestor entirely.
        assert_eq!(bundle_root(Path::new("/Contents/MacOS/bin")), None);
    }

    #[test]
    fn system_install_detection() {
        assert!(is_system_installed(Path::new("/usr/bin/briskablast-launcher")));
        assert!(is_system_installed(Path::new("/usr/local/bin/briskablast-launcher")));
        assert!(!is_system_installed(Path::new(
            "/home/x/apps/briskablast-launcher"
        )));
        // Component-wise prefix match — /usrx must not count.
        assert!(!is_system_installed(Path::new("/usrx/briskablast-launcher")));
    }
}
