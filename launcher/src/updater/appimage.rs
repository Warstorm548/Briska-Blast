//! Linux AppImage self-update.
//!
//! An AppImage runs from a read-only squashfs mount (`current_exe()` points
//! into `/tmp/.mount_…`), so `self_update`'s in-place binary swap can never
//! work there. Instead the *outer* `.AppImage` file — whose path the AppImage
//! runtime exports as env `APPIMAGE` — is replaced: download the new release's
//! AppImage to a dot-prefixed staging name in the same directory, mark it
//! executable, and atomically rename it over the old file. The running
//! process keeps its (now unlinked) old inode, so this is safe while live;
//! the caller exits for relaunch as on every other platform.

use super::asset_fetch;
use std::path::Path;

/// Asset-name suffix the release workflow gives the AppImage artifact
/// (`briskablast-launcher-<ver>-linux.AppImage`).
const ASSET_SUFFIX: &str = "-linux.AppImage";

/// Replace the AppImage at `appimage_path` (env `APPIMAGE`) with `version`'s.
/// On success the file on disk is the new version; caller must exit for
/// relaunch. On any error the old AppImage is untouched and the staging file
/// is removed best-effort.
pub(super) async fn update_appimage(version: &str, appimage_path: &Path) -> Result<(), String> {
    let dir = appimage_path
        .parent()
        .ok_or_else(|| format!("AppImage path {} has no parent", appimage_path.display()))?;
    let file_name = appimage_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("AppImage path {} has no file name", appimage_path.display()))?;
    tracing::info!(target_version = version, path = %appimage_path.display(), "starting AppImage self-update");

    let asset = asset_fetch::find_release_asset(version, ASSET_SUFFIX).await?;
    let staging = dir.join(format!(".{file_name}.staging-{}", uuid::Uuid::new_v4()));

    let result = stage_and_swap(&asset, &staging, appimage_path).await;
    if result.is_err() {
        if let Err(cleanup) = tokio::fs::remove_file(&staging).await {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %cleanup, path = %staging.display(), "could not remove AppImage staging file (non-fatal)");
            }
        }
    }
    result
}

async fn stage_and_swap(
    asset: &asset_fetch::ReleaseAsset,
    staging: &Path,
    appimage_path: &Path,
) -> Result<(), String> {
    asset_fetch::download_to_file(asset, staging, asset_fetch::ELF_MAGIC).await?;

    // The AppImage must be executable or the next launch is a cryptic
    // "permission denied" from the shell / desktop entry.
    let perms = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    tokio::fs::set_permissions(staging, perms)
        .await
        .map_err(|e| format!("set AppImage executable bit: {e}"))?;

    // Same-directory rename: atomic on the same filesystem, and the running
    // process keeps the old (unlinked) inode until it exits.
    tokio::fs::rename(staging, appimage_path)
        .await
        .map_err(|e| format!("swap AppImage into place: {e}"))?;

    tracing::info!(path = %appimage_path.display(), "AppImage self-update swap complete");
    Ok(())
}
