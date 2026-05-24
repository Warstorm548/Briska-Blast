//! Download → extract → manifest for a per-channel game-files release.
//!
//! Picks the platform-appropriate asset from a `GameRelease`, streams the
//! download to a temp file inside the target install dir, extracts the
//! archive, writes `installed.json` so future boots can identify the
//! installed version without re-querying GitHub.

use crate::channel::Channel;
use crate::updater::branches::github::{GameRelease, ReleaseAsset};
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Max time to establish a TCP+TLS connection to GitHub's asset CDN.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Max total time for a single download. Generous to accommodate the
/// expected ~100MB-1GB game artifacts on slow links; trips only when a
/// connection genuinely hangs rather than capping legitimate downloads.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

pub const MANIFEST_FILENAME: &str = "installed.json";

/// On-disk record of what's currently installed in a channel's install dir.
/// Schema is stable across launcher versions; new optional fields only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledManifest {
    /// Semver string matching the GitHub release tag's version component.
    pub version: String,
    /// Channel name, lowercase (`stable` / `ea` / `dev`).
    pub channel: String,
    /// RFC3339 timestamp of when this slot was installed.
    pub installed_at: String,
    /// Path of the game executable RELATIVE to the install dir. The Godot
    /// export drops the binary either at the top level or nested one
    /// directory deep depending on how the artifact was packaged.
    pub executable: String,
}

/// Stream-of-progress emitted by `download_and_install`. The fraction is
/// 0.0..=1.0; Extracting/Done are discrete states with no fraction. Stage 6
/// routes these through `Message::DownloadProgress` into the bottom
/// progress bar widget.
#[derive(Debug, Clone)]
pub enum InstallProgress {
    Downloading {
        fraction: f32,
        bytes_now: u64,
        bytes_total: u64,
    },
    Extracting,
    Done,
}

/// Successful install summary returned to the app.
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub install_dir: PathBuf,
    pub version: String,
    pub executable: String,
}

/// Pick the platform-appropriate asset from a release's asset list. The
/// release workflow names artifacts:
///   `briskablast-client-<channel>-<version>-linux.tar.gz`
///   `briskablast-client-<channel>-<version>-windows.zip`
/// We match on the trailing platform marker so the filename can evolve
/// (e.g. arch suffix) without breaking this.
pub fn select_platform_asset(release: &GameRelease) -> Option<&ReleaseAsset> {
    #[cfg(target_os = "linux")]
    const NEEDLE: &str = "linux.tar.gz";
    #[cfg(target_os = "windows")]
    const NEEDLE: &str = "windows.zip";
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    const NEEDLE: &str = "unsupported";
    // ends_with rather than contains so we don't accidentally pick a
    // companion file like `…linux.tar.gz.sha256` or `…windows.zip.sig` if
    // checksum / signature assets are ever attached alongside the artifact.
    release.assets.iter().find(|a| a.name.ends_with(NEEDLE))
}

/// Download + extract + manifest. The chosen install dir is
/// `<install_root>/<channel.dir_name()>/` and is wiped clean before extract
/// to avoid mixing files from a previous version. `on_progress` is called
/// from this future's executor — the caller should funnel events into an
/// Iced channel for UI updates.
pub async fn download_and_install<F>(
    channel: Channel,
    release: GameRelease,
    install_root: PathBuf,
    on_progress: F,
) -> Result<InstallResult, String>
where
    F: Fn(InstallProgress) + Send + 'static,
{
    let asset = select_platform_asset(&release).ok_or_else(|| {
        format!(
            "no platform-matching asset (linux.tar.gz / windows.zip) in release {}",
            release.tag
        )
    })?;
    let asset_name = asset.name.clone();
    let asset_url = asset.download_url.clone();

    let install_dir = install_root.join(channel.dir_name());
    if install_dir.exists() {
        tokio::fs::remove_dir_all(&install_dir)
            .await
            .map_err(|e| format!("clean install dir: {e}"))?;
    }
    tokio::fs::create_dir_all(&install_dir)
        .await
        .map_err(|e| format!("create install dir: {e}"))?;

    // Download into a temp file at the install dir root; we delete it after
    // a successful extract. Leaving it on failure helps post-mortem debugging.
    let temp_archive = install_dir.join(format!(".download-{asset_name}"));

    let client = reqwest::Client::builder()
        .user_agent("briskablast-launcher")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| format!("http client build: {e}"))?;
    let resp = client
        .get(&asset_url)
        .send()
        .await
        .map_err(|e| format!("download request: {e}"))?
        .error_for_status()
        .map_err(|e| format!("download HTTP error: {e}"))?;

    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&temp_archive)
        .await
        .map_err(|e| format!("create temp archive: {e}"))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("download chunk: {e}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("write chunk: {e}"))?;
        downloaded += bytes.len() as u64;
        let fraction = if total > 0 {
            (downloaded as f32 / total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        on_progress(InstallProgress::Downloading {
            fraction,
            bytes_now: downloaded,
            bytes_total: total,
        });
    }
    file.flush()
        .await
        .map_err(|e| format!("flush archive: {e}"))?;
    drop(file);

    on_progress(InstallProgress::Extracting);

    let install_dir_clone = install_dir.clone();
    let temp_archive_clone = temp_archive.clone();
    let asset_name_clone = asset_name.clone();
    let executable = tokio::task::spawn_blocking(move || {
        extract_archive_blocking(&temp_archive_clone, &install_dir_clone, &asset_name_clone)
    })
    .await
    .map_err(|e| format!("extract join: {e}"))??;

    if let Err(e) = tokio::fs::remove_file(&temp_archive).await {
        tracing::warn!(error = %e, "failed to remove temp archive (non-fatal)");
    }

    let manifest = InstalledManifest {
        version: release.version.to_string(),
        channel: channel.dir_name().to_string(),
        installed_at: Utc::now().to_rfc3339(),
        executable: executable.clone(),
    };
    let manifest_path = install_dir.join(MANIFEST_FILENAME);
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("manifest serialize: {e}"))?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .map_err(|e| format!("manifest write: {e}"))?;

    on_progress(InstallProgress::Done);

    Ok(InstallResult {
        install_dir,
        version: release.version.to_string(),
        executable,
    })
}

fn extract_archive_blocking(archive: &Path, dest: &Path, name: &str) -> Result<String, String> {
    use std::fs::File;
    if name.ends_with(".tar.gz") {
        let f = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest)
            .map_err(|e| format!("tar unpack: {e}"))?;
        find_executable(dest, "BriskaBlast.x86_64")
            .ok_or_else(|| "extracted archive does not contain BriskaBlast.x86_64".to_string())
    } else if name.ends_with(".zip") {
        let f = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let mut z = zip::ZipArchive::new(f).map_err(|e| format!("zip open: {e}"))?;
        z.extract(dest).map_err(|e| format!("zip extract: {e}"))?;
        find_executable(dest, "BriskaBlast.exe")
            .ok_or_else(|| "extracted archive does not contain BriskaBlast.exe".to_string())
    } else {
        Err(format!("unsupported archive extension: {name}"))
    }
}

/// Look for the game executable at the top level of `dir`, falling back to a
/// one-level-deep search in case the archive wraps everything in a single
/// directory (a common Godot export quirk).
fn find_executable(dir: &Path, name: &str) -> Option<String> {
    if dir.join(name).exists() {
        return Some(name.to_string());
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join(name);
            if candidate.exists() {
                return candidate
                    .strip_prefix(dir)
                    .ok()
                    .map(|p| p.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Read `<install_dir>/installed.json`. `Ok(None)` if the file is missing
/// (channel is not installed). Parse errors bubble up so callers can flag a
/// corrupted install rather than silently treating it as a fresh slot.
pub async fn installed_manifest(install_dir: &Path) -> Result<Option<InstalledManifest>, String> {
    let path = install_dir.join(MANIFEST_FILENAME);
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("manifest parse: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("manifest read: {e}")),
    }
}
