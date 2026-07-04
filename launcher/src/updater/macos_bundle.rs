//! macOS whole-bundle self-update.
//!
//! The launcher ships as an ad-hoc-signed `BriskaBlast Launcher.app`. Swapping
//! only the binary inside `Contents/MacOS/` (what `self_update` would do)
//! breaks the bundle's signature seal — on Apple Silicon the next launch is
//! refused as "damaged" — and leaves `Info.plist`/icon stale. So the *entire*
//! bundle is replaced instead, Sparkle-style:
//!
//! 1. download the release's `…-macos-app.tar.gz` (the CI-signed .app) into a
//!    dot-prefixed staging dir **next to** the live bundle (same filesystem ⇒
//!    the final renames are atomic),
//! 2. extract and verify the signature with `codesign` (part of macOS, no
//!    cert needed); re-sign ad-hoc locally if the seal didn't survive,
//! 3. rename the live bundle aside, rename the new one into place (rolling
//!    back on failure), and let the caller exit for relaunch.
//!
//! Because the launcher itself downloads the archive there is **no quarantine
//! xattr**, so Gatekeeper never re-assesses the updated app — the one-time
//! right-click-open only ever applies to the original .dmg install. The
//! swapped-aside old bundle (which this process is still running from) is
//! mopped up by `cleanup.rs` on the next launch.

use super::asset_fetch;
use std::path::{Path, PathBuf};

/// Asset-name suffix the release workflow gives the signed-bundle tarball
/// (`briskablast-launcher-<ver>-macos-app.tar.gz`). Deliberately does not
/// contain the target triple so `self_update`'s triple-matching bare-binary
/// fallback can never pick it.
const ASSET_SUFFIX: &str = "-macos-app.tar.gz";

/// Replace the live bundle at `bundle` with `version`'s. On success the .app
/// on disk is the new version; caller must exit for relaunch. On any error
/// the live bundle is untouched and staging is removed best-effort.
pub(super) async fn update_bundle(version: &str, bundle: &Path) -> Result<(), String> {
    let parent = bundle
        .parent()
        .ok_or_else(|| format!("bundle {} has no parent directory", bundle.display()))?;
    let bundle_name = bundle
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("bundle {} has no file name", bundle.display()))?;
    tracing::info!(target_version = version, bundle = %bundle.display(), "starting macOS bundle self-update");

    let asset = asset_fetch::find_release_asset(version, ASSET_SUFFIX).await?;
    let staging = parent.join(format!(".{bundle_name}.staging-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| format!("create staging dir: {e}"))?;

    let result = stage_and_swap(&asset, &staging, parent, bundle, bundle_name).await;
    if result.is_err() {
        // Best-effort: the live bundle is untouched on every error path, a
        // leaked staging dir is just disk noise (and next-launch cleanup
        // removes it anyway).
        if let Err(cleanup) = tokio::fs::remove_dir_all(&staging).await {
            tracing::warn!(error = %cleanup, path = %staging.display(), "could not remove bundle staging dir (non-fatal)");
        }
    }
    result
}

async fn stage_and_swap(
    asset: &asset_fetch::ReleaseAsset,
    staging: &Path,
    parent: &Path,
    bundle: &Path,
    bundle_name: &str,
) -> Result<(), String> {
    let tarball = staging.join(&asset.name);
    asset_fetch::download_to_file(asset, &tarball, asset_fetch::GZIP_MAGIC).await?;

    // Extraction + codesign are blocking work.
    let staging_owned = staging.to_path_buf();
    let new_app = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        extract_tarball(&tarball, &staging_owned)?;
        let app = find_extracted_app(&staging_owned)?;
        ensure_adhoc_signature(&app)?;
        Ok(app)
    })
    .await
    .map_err(|e| format!("bundle stage join error: {e}"))??;

    // Swap. Same shape as the game installer's staging→install commit
    // (branches/installer/download.rs): move the live bundle aside first so a
    // failed second rename can roll back.
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let aside = parent.join(format!(".{bundle_name}.old-{stamp}"));
    tokio::fs::rename(bundle, &aside)
        .await
        .map_err(|e| format!("move current app aside: {e}"))?;
    if let Err(e) = tokio::fs::rename(&new_app, bundle).await {
        if let Err(restore) = tokio::fs::rename(&aside, bundle).await {
            tracing::error!(
                error = %restore,
                aside = %aside.display(),
                bundle = %bundle.display(),
                "FAILED to restore old app after swap failure"
            );
        }
        return Err(format!("swap new app into place: {e}"));
    }

    // The aside dir contains the binary this process is running from, so it
    // is left for next-launch cleanup; the staging dir (now just the tarball)
    // can go immediately.
    if let Err(e) = tokio::fs::remove_dir_all(staging).await {
        tracing::warn!(error = %e, path = %staging.display(), "could not remove staging dir after swap (non-fatal)");
    }
    tracing::info!(bundle = %bundle.display(), "macOS bundle self-update swap complete");
    Ok(())
}

fn extract_tarball(tarball: &Path, dest: &Path) -> Result<(), String> {
    let file =
        std::fs::File::open(tarball).map_err(|e| format!("open downloaded tarball: {e}"))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dest).map_err(|e| format!("tar unpack: {e}"))
}

/// The tarball contains exactly one top-level `<name>.app` directory.
fn find_extracted_app(dir: &Path) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read staging dir: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.extension().is_some_and(|e| e == "app") {
            return Ok(path);
        }
    }
    Err("downloaded archive contains no .app bundle".into())
}

/// Verify the ad-hoc seal survived the tar round-trip (codesign signatures
/// live in the Mach-O + `_CodeSignature`, so it should have); if not, re-sign
/// ad-hoc locally — `codesign --sign -` needs no certificate — and verify
/// again. Abort (live bundle untouched) if it still fails.
fn ensure_adhoc_signature(app: &Path) -> Result<(), String> {
    if codesign_verify(app)? {
        return Ok(());
    }
    tracing::warn!(app = %app.display(), "extracted app failed codesign verify — re-signing ad-hoc");
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app)
        .output()
        .map_err(|e| format!("run codesign --sign: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ad-hoc re-sign failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if codesign_verify(app)? {
        Ok(())
    } else {
        Err("downloaded app still fails signature verification after ad-hoc re-sign".into())
    }
}

fn codesign_verify(app: &Path) -> Result<bool, String> {
    let output = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .map_err(|e| format!("run codesign --verify: {e}"))?;
    if !output.status.success() {
        tracing::warn!(
            app = %app.display(),
            status = %output.status,
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "codesign verify failed"
        );
    }
    Ok(output.status.success())
}
