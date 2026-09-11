//! Bare-binary self-update: download the platform asset, extract the new
//! executable, and swap it over the running one with the rename trick.
//!
//! Replaces `self_update`'s `Update::update()`, which did exactly this but as a
//! single opaque blocking call with no progress callback of any kind. The
//! launcher now shows a stepped progress bar for its own update, which that
//! call cannot feed, so the same work is assembled here out of parts that
//! already existed: [`super::asset_fetch`] for the rate-limit-aware streaming
//! download, and `self_replace` — which `self_update` re-exports and which is
//! the very crate it delegated the swap to — for the swap itself. The on-disk
//! outcome is identical; only the visibility changed.
//!
//! Used for Windows, and as the fallback on macOS/Linux for a bare binary that
//! is not in a `.app` bundle or an AppImage. Those two have their own modules
//! because their swap is not a binary swap at all.

use super::asset_fetch;
use crate::updater::branches::InstallProgress;
use crate::updater::plan::Phase;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Release-asset suffix and the executable's name inside it, per platform.
/// Mirrors the artifact names in `release-launcher.yml`.
#[cfg(target_os = "windows")]
const ASSET_SUFFIX: &str = "-x86_64-pc-windows-msvc.zip";
#[cfg(target_os = "linux")]
const ASSET_SUFFIX: &str = "-x86_64-unknown-linux-gnu.tar.gz";
#[cfg(target_os = "macos")]
const ASSET_SUFFIX: &str = "-aarch64-apple-darwin.tar.gz";

#[cfg(target_os = "windows")]
const EXE_NAME: &str = "briskablast-launcher.exe";
#[cfg(not(target_os = "windows"))]
const EXE_NAME: &str = "briskablast-launcher";

#[cfg(target_os = "windows")]
const EXPECTED_MAGIC: &[u8] = asset_fetch::ZIP_MAGIC;
#[cfg(target_os = "linux")]
const EXPECTED_MAGIC: &[u8] = &[0x1f, 0x8b];
#[cfg(target_os = "macos")]
const EXPECTED_MAGIC: &[u8] = asset_fetch::GZIP_MAGIC;

/// Download `version`'s binary asset and swap it over the running executable.
///
/// On success the launcher on disk is the new version and the caller must not
/// continue running the old code. On any failure the running executable is
/// untouched — everything happens in a staging directory that is removed either
/// way.
pub(super) async fn swap_binary<F>(version: &str, on_progress: &Arc<F>) -> Result<(), String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
{
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| format!("executable path {} has no parent", exe.display()))?;

    // Stage beside the running executable so the swap is a same-volume rename
    // rather than a cross-device copy, and so a failure leaves its debris in
    // one removable directory.
    let staging = dir.join(format!(".briskablast-launcher.staging-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| format!("create self-update staging dir: {e}"))?;

    let result = stage_and_swap(version, &staging, on_progress).await;

    if let Err(e) = tokio::fs::remove_dir_all(&staging).await {
        // Non-fatal either way: on success the swap already happened, and on
        // failure the running binary is what matters. Windows can also hold the
        // extracted file briefly after the swap.
        tracing::warn!(
            error = %e,
            path = %staging.display(),
            "could not remove self-update staging dir (non-fatal)"
        );
    }
    result
}

async fn stage_and_swap<F>(
    version: &str,
    staging: &Path,
    on_progress: &Arc<F>,
) -> Result<(), String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
{
    let asset = asset_fetch::find_release_asset(version, ASSET_SUFFIX).await?;
    let archive = staging.join(&asset.name);

    let cb = Arc::clone(on_progress);
    asset_fetch::download_to_file(&asset, &archive, EXPECTED_MAGIC, move |now, total| {
        let fraction = if total > 0 {
            (now as f32 / total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        cb(InstallProgress::Phase {
            phase: Phase::Downloading,
            fraction,
            bytes_now: now,
            bytes_total: total,
        });
    })
    .await?;

    on_progress(InstallProgress::Phase {
        phase: Phase::Installing,
        fraction: 0.0,
        bytes_now: 0,
        bytes_total: 0,
    });

    let staging_owned = staging.to_path_buf();
    let archive_owned = archive.clone();
    let new_exe = tokio::task::spawn_blocking(move || {
        extract_executable_blocking(&archive_owned, &staging_owned)
    })
    .await
    .map_err(|e| format!("self-update extract join: {e}"))??;

    // The rename trick. On Windows this moves the running executable aside and
    // puts the new one in its place, leaving a `.__relocated__.exe` that
    // `cleanup_stale_update_artifacts` mops up on the next launch — which is
    // why the relaunched process must wait for this one to exit first.
    tokio::task::spawn_blocking(move || self_update::self_replace::self_replace(&new_exe))
        .await
        .map_err(|e| format!("self-replace join: {e}"))?
        .map_err(|e| format!("replace running executable: {e}"))?;

    on_progress(InstallProgress::Phase {
        phase: Phase::Installing,
        fraction: 1.0,
        bytes_now: 0,
        bytes_total: 0,
    });
    tracing::info!(version, "self-update binary swap complete");
    Ok(())
}

/// Pull the launcher executable out of the downloaded archive into `dest`,
/// returning its path. Blocking; call inside `spawn_blocking`.
fn extract_executable_blocking(archive: &Path, dest: &Path) -> Result<PathBuf, String> {
    let out = dest.join(EXE_NAME);

    #[cfg(target_os = "windows")]
    {
        let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip open: {e}"))?;
        let mut entry = zip
            .by_name(EXE_NAME)
            .map_err(|e| format!("{EXE_NAME} not in self-update archive: {e}"))?;
        let mut writer =
            std::fs::File::create(&out).map_err(|e| format!("create extracted exe: {e}"))?;
        std::io::copy(&mut entry, &mut writer)
            .map_err(|e| format!("write extracted exe: {e}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest).map_err(|e| format!("tar unpack: {e}"))?;
        if !out.is_file() {
            return Err(format!(
                "{EXE_NAME} not found in the extracted self-update archive"
            ));
        }
        // The archive should carry the exec bit, but a tar built without it
        // would produce a launcher that cannot start — and the failure would
        // only surface after the swap. Set it explicitly.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("set exec bit on new launcher: {e}"))?;
    }

    Ok(out)
}
