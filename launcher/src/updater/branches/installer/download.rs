//! Download → extract → manifest for a per-channel game-files release.
//!
//! Picks the platform-appropriate asset from a `GameRelease`, streams the
//! download to a temp file inside a staging dir, extracts the archive, and
//! writes `installed.json` so future boots can identify the installed version
//! without re-querying GitHub. The final staging → install-dir swap is atomic.

use crate::channel::Channel;
use crate::updater::branches::github::{GameRelease, ReleaseAsset};
use chrono::Utc;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use super::extract::extract_archive_blocking;
use super::manifest::{InstalledManifest, MANIFEST_FILENAME};

/// Max time to establish a TCP+TLS connection to GitHub's asset CDN.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Max total time for a single download. Generous to accommodate the
/// expected ~100MB-1GB game artifacts on slow links; trips only when a
/// connection genuinely hangs rather than capping legitimate downloads.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

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
    #[cfg(target_os = "macos")]
    const NEEDLE: &str = "macos.tar.gz";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
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
            "no platform-matching asset (linux.tar.gz / windows.zip / macos.tar.gz) in release {}",
            release.tag
        )
    })?;
    let asset_name = asset.name.clone();
    let asset_url = asset.download_url.clone();

    // Transactional install: all destructive work happens in a uuid-suffixed
    // STAGING sibling of the final install dir. A mid-download / mid-extract
    // failure leaves the live install on disk untouched. Only the final
    // rename (staging → install_dir) commits the new version, with the prior
    // install moved aside first so we can roll back if that rename itself
    // fails. Both sides of the swap live under `install_root` so they share
    // a filesystem and the renames are atomic.
    let final_install_dir = install_root.join(channel.dir_name());
    let staging_dir = install_root.join(format!(
        ".{}.staging-{}",
        channel.dir_name(),
        uuid::Uuid::new_v4()
    ));

    let executable: String = match stage_install(
        &release,
        channel.dir_name(),
        &asset_name,
        &asset_url,
        &staging_dir,
        &on_progress,
    )
    .await
    {
        Ok(exe) => exe,
        Err(e) => {
            // Best-effort cleanup. Leaving the staging dir behind is
            // worse than the alternative — but the live install dir is
            // untouched, which is the load-bearing invariant here.
            if let Err(cleanup) = tokio::fs::remove_dir_all(&staging_dir).await {
                tracing::warn!(
                    error = %cleanup,
                    path = %staging_dir.display(),
                    "failed to clean staging dir after install error (non-fatal)"
                );
            }
            return Err(e);
        }
    };

    // Atomic swap. If a prior install exists, move it aside under a
    // dot-prefixed name first; on a failed swap we put it back. Both
    // renames are atomic on the same filesystem.
    let had_prior = final_install_dir.exists();
    let old_aside = if had_prior {
        let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
        let aside =
            install_root.join(format!(".{}.old-{stamp}", channel.dir_name()));
        if let Err(e) = tokio::fs::rename(&final_install_dir, &aside).await {
            // The staged install is ready but we couldn't move the live install
            // aside to make room for the swap. Clean up the staging tree before
            // returning — the live install is untouched — matching the other
            // install-error paths rather than leaking the staging dir.
            let _ = tokio::fs::remove_dir_all(&staging_dir).await;
            return Err(format!("move old install aside: {e}"));
        }
        Some(aside)
    } else {
        None
    };

    if let Err(e) = tokio::fs::rename(&staging_dir, &final_install_dir).await {
        // Roll back: restore the old install.
        if let Some(aside) = &old_aside {
            if let Err(restore) = tokio::fs::rename(aside, &final_install_dir).await {
                tracing::error!(
                    error = %restore,
                    aside = %aside.display(),
                    install_dir = %final_install_dir.display(),
                    "FAILED to restore old install after staging-swap failure"
                );
            }
        }
        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
        return Err(format!("swap staging \u{2192} install dir: {e}"));
    }

    // Clean up the moved-aside old install. Best effort — the new install
    // already succeeded so a lingering `.channel.old-<stamp>` dir is just
    // disk noise (dot-prefixed, not user-visible).
    if let Some(aside) = old_aside {
        if let Err(e) = tokio::fs::remove_dir_all(&aside).await {
            tracing::warn!(
                error = %e,
                aside = %aside.display(),
                "failed to remove old install dir after swap (non-fatal)"
            );
        }
    }

    on_progress(InstallProgress::Done);

    Ok(InstallResult {
        install_dir: final_install_dir,
        version: release.version.to_string(),
        executable,
    })
}

/// Inner stage of `download_and_install` — does the download, extraction,
/// and manifest write into `staging_dir`. Returns the resolved executable's
/// relative path on success. Any error is propagated unchanged; cleanup of
/// `staging_dir` is the caller's responsibility.
async fn stage_install<F>(
    release: &GameRelease,
    channel_dir_name: &str,
    asset_name: &str,
    asset_url: &str,
    staging_dir: &Path,
    on_progress: &F,
) -> Result<String, String>
where
    F: Fn(InstallProgress) + Send + 'static,
{
    tokio::fs::create_dir_all(staging_dir)
        .await
        .map_err(|e| format!("create staging dir: {e}"))?;

    let temp_archive = staging_dir.join(format!(".download-{asset_name}"));

    // Rate-limit gate: the asset endpoint is a counted core-API request, so it
    // honours the same back-off the discovery checks do. A closed gate yields a
    // clean "resumes at HH:MM" instead of letting the install start and die
    // mid-flight on a 403.
    if let crate::ratelimit::Gate::Blocked { resume_at } = crate::ratelimit::gate() {
        return Err(format!(
            "GitHub rate limit reached \u{2014} install resumes at {}.",
            crate::ratelimit::format_resume(resume_at)
        ));
    }

    let client = reqwest::Client::builder()
        .user_agent("briskablast-launcher")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| format!("http client build: {e}"))?;
    // `asset_url` is the GitHub REST API endpoint
    // (https://api.github.com/repos/.../releases/assets/<id>), inherited
    // unchanged from self_update::backends::github's `asset["url"]` parser.
    // Without the Accept header below the API returns the asset's JSON
    // metadata (~few hundred bytes) instead of the binary, which silently
    // gets saved as `.download-foo.zip` and later surfaces as a confusing
    // "zip open: Could not find EOCD" error in the extractor. This header
    // mirrors what self_update itself sets at update.rs:234 in its own
    // (working) launcher self-update path. See:
    //   https://docs.github.com/en/rest/releases/assets
    let resp = client
        .get(asset_url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("download request: {e}"))?;
    // A rate-limit comes back as a *direct* 403/429 from api.github.com, whose
    // rate-limit headers are readable here. A success is a 302 to the CDN that
    // reqwest already followed, whose final headers are the CDN's and carry no
    // GitHub budget — so we only act on the rate-limit case (the gate above plus
    // the release-list Layer B cover proactive back-off; there's no budget to
    // record off a CDN 200).
    if let crate::updater::github_client::RateSignal::Limited { reset } =
        crate::updater::github_client::inspect(resp.status(), resp.headers())
    {
        let resume_at = crate::ratelimit::note_rate_limited(reset);
        return Err(format!(
            "GitHub rate limit reached \u{2014} install resumes at {}.",
            crate::ratelimit::format_resume(resume_at)
        ));
    }
    let resp = resp
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
    // Flush and explicitly close the file BEFORE handing off to the
    // (blocking) extractor. On Windows in particular, `drop(file)` on a
    // tokio handle doesn't guarantee the underlying file is fully closed
    // by the time spawn_blocking re-opens it for reading — sync_all()
    // + shutdown() does. Without this, a fast extract could observe a
    // truncated file even though all bytes were written.
    file.flush()
        .await
        .map_err(|e| format!("flush archive: {e}"))?;
    file.sync_all()
        .await
        .map_err(|e| format!("sync archive: {e}"))?;
    file.shutdown()
        .await
        .map_err(|e| format!("close archive: {e}"))?;
    drop(file);

    // Verify the file on disk is actually an archive of the expected kind
    // BEFORE handing it to the extractor. This supersedes v0.8.1's
    // Content-Length-based truncation checks — those were guards for a
    // narrower failure mode (mid-stream truncation with a known total)
    // and didn't fire on the real-world bug, where `total = 0` and the
    // file was a small JSON metadata response from the GitHub API
    // (missing Accept header — now fixed above). A 4-byte magic-byte
    // check covers BOTH cases: truncated AND wrong-content.
    //
    //   zip   = PK\x03\x04  (50 4b 03 04)
    //   gzip  = 1f 8b
    //
    // ZIP signature ref: https://en.wikipedia.org/wiki/ZIP_(file_format)
    let on_disk = tokio::fs::metadata(&temp_archive)
        .await
        .map(|m| m.len())
        .map_err(|e| format!("stat temp archive: {e}"))?;
    {
        use tokio::io::AsyncReadExt;
        let mut head = [0u8; 4];
        let mut f = tokio::fs::File::open(&temp_archive)
            .await
            .map_err(|e| format!("open temp archive for magic check: {e}"))?;
        let n = f
            .read(&mut head)
            .await
            .map_err(|e| format!("read temp archive magic: {e}"))?;
        if n < 4 {
            return Err(format!(
                "downloaded archive is only {n} bytes — far smaller than \
                 the expected game asset. Likely an error response or \
                 stub instead of the real binary."
            ));
        }
        let expected: &[u8] = if asset_name.ends_with(".tar.gz") {
            &[0x1f, 0x8b]
        } else if asset_name.ends_with(".zip") {
            &[0x50, 0x4b, 0x03, 0x04]
        } else {
            &[]
        };
        if !expected.is_empty() && !head.starts_with(expected) {
            // Include a sample of the file content so the next failure
            // report is self-diagnostic (`{"url":...}` ⇒ JSON metadata
            // from a missing Accept header; `<html>` ⇒ HTML error page).
            let sample = tokio::fs::read(&temp_archive)
                .await
                .map(|b| {
                    String::from_utf8_lossy(&b[..b.len().min(256)]).into_owned()
                })
                .unwrap_or_default();
            let kind = if asset_name.ends_with(".tar.gz") {
                "tar.gz"
            } else {
                "zip"
            };
            return Err(format!(
                "downloaded content is not a recognised {kind} archive \
                 (first 4 bytes: {head:02x?}, on-disk size: {on_disk}, \
                 sample: {sample:?})"
            ));
        }
    }
    tracing::info!(
        downloaded,
        total,
        on_disk,
        archive = %temp_archive.display(),
        "download complete, magic bytes ok, handing off to extractor"
    );

    on_progress(InstallProgress::Extracting);

    let staging_clone = staging_dir.to_path_buf();
    let temp_archive_clone = temp_archive.clone();
    let asset_name_clone = asset_name.to_string();
    let executable = tokio::task::spawn_blocking(move || {
        extract_archive_blocking(&temp_archive_clone, &staging_clone, &asset_name_clone)
    })
    .await
    .map_err(|e| format!("extract join: {e}"))??;

    if let Err(e) = tokio::fs::remove_file(&temp_archive).await {
        tracing::warn!(error = %e, "failed to remove temp archive (non-fatal)");
    }

    let manifest = InstalledManifest {
        version: release.version.to_string(),
        channel: channel_dir_name.to_string(),
        installed_at: Utc::now().to_rfc3339(),
        executable: executable.clone(),
    };
    let manifest_path = staging_dir.join(MANIFEST_FILENAME);
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("manifest serialize: {e}"))?;
    tokio::fs::write(&manifest_path, manifest_json)
        .await
        .map_err(|e| format!("manifest write: {e}"))?;

    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::branches::github::{GameRelease, ReleaseAsset};

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            download_url: format!("https://example.test/{name}"),
        }
    }

    /// Whatever this platform's needle is, the real archive must be chosen over a
    /// checksum/signature companion whose name also contains the needle (the
    /// `ends_with` guard). Runs on the supported targets only.
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    #[test]
    fn select_platform_asset_prefers_archive_over_companion() {
        let release = GameRelease {
            version: semver::Version::parse("0.2.5").unwrap(),
            tag: "game-v0.2.5-dev.1".to_string(),
            body: String::new(),
            assets: vec![
                asset("briskablast-client-dev-0.2.5-linux.tar.gz"),
                asset("briskablast-client-dev-0.2.5-linux.tar.gz.sha256"),
                asset("briskablast-client-dev-0.2.5-windows.zip"),
                asset("briskablast-client-dev-0.2.5-windows.zip.sig"),
                asset("briskablast-client-dev-0.2.5-macos.tar.gz"),
                asset("briskablast-client-dev-0.2.5-macos.tar.gz.sha256"),
            ],
        };
        let picked = select_platform_asset(&release).expect("a supported target should match");
        // Never a companion file.
        assert!(!picked.name.ends_with(".sha256") && !picked.name.ends_with(".sig"));
        // And it matches this platform's real archive suffix.
        #[cfg(target_os = "linux")]
        assert!(picked.name.ends_with("linux.tar.gz"));
        #[cfg(target_os = "windows")]
        assert!(picked.name.ends_with("windows.zip"));
        #[cfg(target_os = "macos")]
        assert!(picked.name.ends_with("macos.tar.gz"));
    }
}
