//! GitHub-Releases-backed self-update via the `self_update` crate.
//!
//! Discovery: list all releases on `Warstorm548/Briska-Blast`, keep ones whose
//! tag begins with `launcher-v`, parse the suffix as semver, pick the highest
//! one greater than the running version.
//!
//! Application is per-OS (dispatched in [`run_self_update`]):
//! * **Windows** — the rename trick ([`super::binary_swap`]): rename the
//!   running exe, drop the downloaded one in its place.
//! * **macOS** — whole-bundle swap ([`super::macos_bundle`]) when running from
//!   a `.app`; the rename trick only for a bare binary.
//! * **Linux** — outer-file replacement ([`super::appimage`]) when running as
//!   an AppImage; the rename trick for a user-writable bare binary; a
//!   "download the new .deb" message for system-wide (`/usr/…`) installs.
//!
//! In every case the caller must start the replacement process
//! ([`super::relaunch`]) and then `std::process::exit(0)` immediately after a
//! successful swap; leftovers are cleaned up by `cleanup.rs` on the next run,
//! which is why the replacement waits for this process to exit first.

use super::github_client;
use super::release_cache::{self, Freshness};
use crate::updater::branches::InstallProgress;
use semver::Version;
use std::path::Path;
use std::sync::Arc;

pub(super) const REPO_OWNER: &str = "Warstorm548";
pub(super) const REPO_NAME: &str = "Briska-Blast";
pub(super) const TAG_PREFIX: &str = "launcher-v";

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
/// Suitable for `iced::Task::perform`.
///
/// Goes through `release_cache`, so the boot check shares one request with every
/// channel's `latest_release` while the rate-limit safety net still sees the
/// response status and headers. Pass [`Freshness::Cached`] on boot and
/// [`Freshness::Revalidate`] for the user-pressed "Check for Updates"; a closed
/// gate or a confirmed `403`/`429` surfaces as the user-facing rate-limit message.
pub async fn check_for_update(freshness: Freshness) -> Result<UpdateCheckOutcome, String> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("invalid current version {:?}: {e}", env!("CARGO_PKG_VERSION")))?;
    tracing::debug!(%current, "querying GitHub Releases for launcher updates");

    let releases = release_cache::releases(REPO_OWNER, REPO_NAME, freshness)
        .await
        .map_err(|e| e.to_user_string())?;

    let mut best: Option<(Version, &github_client::Release)> = None;
    for r in releases.iter() {
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
/// module docs), reporting progress through `on_progress`.
///
/// Returns Ok(()) on a successful swap; the caller MUST then start the
/// replacement (`super::relaunch::spawn_replacement`) and
/// `std::process::exit(0)`, because the launcher on disk has been replaced and
/// this process is still running the old code.
///
/// `on_progress` emits the same [`InstallProgress`] events the game installer
/// does, so both drive the one bottom-bar plan renderer.
pub async fn run_self_update<F>(version: String, on_progress: F) -> Result<(), String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
{
    let on_progress = Arc::new(on_progress);

    #[cfg(target_os = "macos")]
    {
        // Installed the normal way (.dmg → .app), the running exe is at
        // <bundle>.app/Contents/MacOS/…: replace the whole bundle so the
        // ad-hoc signature seal stays valid. A bare binary (no bundle)
        // falls through to the binary swap below.
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        if let Some(bundle) = bundle_root(&exe) {
            return super::macos_bundle::update_bundle(&version, &bundle, &on_progress).await;
        }
    }
    #[cfg(target_os = "linux")]
    {
        // AppImage: the exe lives in a read-only squashfs mount, so the
        // outer .AppImage file (env set by the AppImage runtime) is
        // replaced instead of the running binary.
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            let path = std::path::PathBuf::from(appimage);
            return super::appimage::update_appimage(&version, &path, &on_progress).await;
        }
        // System-wide (.deb) install: refuse up front with a useful message
        // instead of letting the swap die on permission-denied.
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        if is_system_installed(&exe) {
            tracing::warn!(exe = %exe.display(), "refusing self-update of system-wide install");
            return Err(SYSTEM_INSTALL_MSG.into());
        }
    }

    let result = super::binary_swap::swap_binary(&version, &on_progress).await;
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
