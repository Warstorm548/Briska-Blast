//! Download → extract → manifest for a per-channel game-files release.
//!
//! Picks the platform-appropriate asset from a `GameRelease`, streams the
//! download to a temp file inside a staging dir, extracts the archive, and
//! writes `installed.json` so future boots can identify the installed version
//! without re-querying GitHub. The final staging → install-dir swap is atomic.

use crate::channel::Channel;
use crate::updater::branches::github::{GameRelease, ReleaseAsset};
use crate::updater::plan::Phase;
use chrono::Utc;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

/// Stream-of-progress emitted by `download_and_install`, routed through
/// `Message::DownloadProgress` into the bottom progress bar.
///
/// Events name their [`Phase`] rather than a step number. The step *number* is a
/// property of the [`crate::updater::plan::UpdatePlan`] the app built for this
/// job, and having the installer also count steps would mean two places had to
/// agree on the same ordering. The app resolves phase → index against the plan
/// instead (`UpdatePlan::index_of_phase`).
///
/// When per-component updates arrive this gains a `component` field and the
/// lookup becomes `index_of(phase, component)`; nothing else about the shape
/// has to change.
#[derive(Debug, Clone)]
pub enum InstallProgress {
    Phase {
        phase: Phase,
        /// Progress within this phase only, `0.0..=1.0`. Resets at each phase
        /// boundary — the bar's continuity is the plan's job, not this event's.
        fraction: f32,
        /// Bytes moved and the expected total for this phase. Both `0` when
        /// unknown (a server that omitted `Content-Length`, or a phase with no
        /// meaningful byte measure); the UI drops the byte readout rather than
        /// printing a total of zero.
        bytes_now: u64,
        bytes_total: u64,
    },
    Done,
}

/// How often the extraction poller re-measures the staging tree. Extraction
/// runs inside an opaque `tar.unpack` / `zip.extract` call with no callback of
/// its own, so bytes-landed-on-disk is measured from outside instead. Frequent
/// enough to look live, rare enough that the directory walk is free.
const EXTRACT_POLL: Duration = Duration::from_millis(200);

/// Successful install summary returned to the app.
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub install_dir: PathBuf,
    pub version: String,
    pub executable: String,
}

/// Suffix of the standalone integrity-manifest asset for this platform.
///
/// The same `files.json` also ships *inside* the archive, where `verify` reads
/// it from. This second, standalone copy exists so the launcher can know the
/// real uncompressed size of an install **before** starting the download the
/// number is meant to describe. It is a few kilobytes.
#[cfg(target_os = "linux")]
const FILES_MANIFEST_ASSET_SUFFIX: &str = "-linux-files.json";
#[cfg(target_os = "windows")]
const FILES_MANIFEST_ASSET_SUFFIX: &str = "-windows-files.json";
#[cfg(target_os = "macos")]
const FILES_MANIFEST_ASSET_SUFFIX: &str = "-macos-files.json";

/// Total uncompressed bytes this release installs, from its standalone
/// `files.json` asset.
///
/// `None` when the release predates the standalone manifest, when the fetch
/// fails, or when the manifest does not parse. Every one of those is a normal,
/// non-fatal outcome: the caller falls back to estimating from the compressed
/// size, and the only consequence is a slightly less evenly-paced progress bar.
/// Nothing here may ever fail an install.
pub async fn fetch_installed_bytes(release: &GameRelease) -> Option<u64> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(FILES_MANIFEST_ASSET_SUFFIX))?;

    // Respect the back-off like any other counted request, but never surface it
    // as an error — a closed gate just means we estimate instead.
    if matches!(crate::ratelimit::gate(), crate::ratelimit::Gate::Blocked { .. }) {
        tracing::debug!("rate-limit gate closed — estimating install size instead");
        return None;
    }

    let client = reqwest::Client::builder()
        .user_agent("briskablast-launcher")
        .connect_timeout(CONNECT_TIMEOUT)
        // A few KB: a short timeout keeps a hung CDN from delaying the install
        // it is only meant to describe.
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;

    let resp = client
        .get(&asset.download_url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .ok()?;
    if let crate::updater::github_client::RateSignal::Limited { reset } =
        crate::updater::github_client::inspect(resp.status(), resp.headers())
    {
        // Still worth recording so the shared back-off learns about it.
        crate::ratelimit::note_rate_limited(reset);
        return None;
    }
    let body = resp.error_for_status().ok()?.text().await.ok()?;

    let manifest: super::manifest::FilesManifest = serde_json::from_str(&body)
        .map_err(|e| tracing::debug!(error = %e, "standalone files.json did not parse"))
        .ok()?;
    if manifest.schema != super::manifest::FILES_MANIFEST_SCHEMA {
        tracing::debug!(schema = manifest.schema, "unsupported files.json schema in release asset");
        return None;
    }
    let total: u64 = manifest.files.values().map(|e| e.size).sum();
    tracing::info!(
        asset = %asset.name,
        files = manifest.files.len(),
        installed_bytes = total,
        "resolved real install size from the release manifest"
    );
    Some(total)
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
    expected_installed_bytes: u64,
    on_progress: F,
) -> Result<InstallResult, String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
{
    // Shared with the extraction poller, which runs as its own task while the
    // blocking extract occupies a worker thread.
    let on_progress = Arc::new(on_progress);
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
        expected_installed_bytes,
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
    expected_installed_bytes: u64,
    on_progress: &Arc<F>,
) -> Result<String, String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
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
        on_progress(InstallProgress::Phase {
            phase: Phase::Downloading,
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

    on_progress(InstallProgress::Phase {
        phase: Phase::Installing,
        fraction: 0.0,
        bytes_now: 0,
        bytes_total: expected_installed_bytes,
    });

    // Extraction progress is measured from outside rather than from within.
    // `tar.unpack` and `zip.extract` are single opaque calls with no callback,
    // and re-implementing them entry-by-entry to get one would put the macOS
    // bundle's symlinks and exec bits — which its ad-hoc signature depends on —
    // at risk for a cosmetic gain. Polling how many bytes have landed in the
    // staging tree gives the same number without touching the extractor at all,
    // and the denominator is the manifest's real uncompressed total.
    let extract_poller = {
        let cb = Arc::clone(on_progress);
        let dir = staging_dir.to_path_buf();
        // The archive is still sitting inside the staging dir while it is being
        // extracted; counting it would report progress before any file landed.
        let skip = temp_archive
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(EXTRACT_POLL).await;
                let dir = dir.clone();
                let skip = skip.clone();
                let Ok(written) =
                    tokio::task::spawn_blocking(move || dir_size_blocking(&dir, &skip)).await
                else {
                    return;
                };
                let fraction = if expected_installed_bytes > 0 {
                    (written as f32 / expected_installed_bytes as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                cb(InstallProgress::Phase {
                    phase: Phase::Installing,
                    fraction,
                    bytes_now: written,
                    bytes_total: expected_installed_bytes,
                });
            }
        })
    };

    let staging_clone = staging_dir.to_path_buf();
    let temp_archive_clone = temp_archive.clone();
    let asset_name_clone = asset_name.to_string();
    let extracted = tokio::task::spawn_blocking(move || {
        extract_archive_blocking(&temp_archive_clone, &staging_clone, &asset_name_clone)
    })
    .await;

    // Await the abort rather than just firing it, so the poller is provably
    // stopped before the next phase begins. A stray late `Installing` event
    // arriving after `Verifying` had started would make the bar jump backwards.
    extract_poller.abort();
    let _ = extract_poller.await;

    let executable = extracted.map_err(|e| format!("extract join: {e}"))??;
    on_progress(InstallProgress::Phase {
        phase: Phase::Installing,
        fraction: 1.0,
        bytes_now: expected_installed_bytes,
        bytes_total: expected_installed_bytes,
    });

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

    verify_staged(staging_dir, expected_installed_bytes, on_progress).await?;

    Ok(executable)
}

/// Hash the staged tree against its own `files.json` before it is swapped in.
///
/// Deliberately runs on **staging**, not on the live install. Verifying after
/// the swap would only tell the user that something broken had already replaced
/// something that worked, and undoing that needs rollback machinery. Verifying
/// here means a corrupt download fails the install through the caller's existing
/// staging-cleanup path, with the previous version still in place and untouched.
async fn verify_staged<F>(
    staging_dir: &Path,
    expected_installed_bytes: u64,
    on_progress: &Arc<F>,
) -> Result<(), String>
where
    F: Fn(InstallProgress) + Send + Sync + 'static,
{
    on_progress(InstallProgress::Phase {
        phase: Phase::Verifying,
        fraction: 0.0,
        bytes_now: 0,
        bytes_total: expected_installed_bytes,
    });

    let cb = Arc::clone(on_progress);
    let outcome = super::verify::verify_install_with_progress(
        staging_dir.to_path_buf(),
        move |hashed, total| {
            let fraction = if total > 0 {
                (hashed as f32 / total as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            cb(InstallProgress::Phase {
                phase: Phase::Verifying,
                fraction,
                bytes_now: hashed,
                bytes_total: total,
            });
        },
    )
    .await;

    match outcome {
        super::VerifyOutcome::Ok { .. } => {
            on_progress(InstallProgress::Phase {
                phase: Phase::Verifying,
                fraction: 1.0,
                bytes_now: expected_installed_bytes,
                bytes_total: expected_installed_bytes,
            });
            Ok(())
        }
        // A pre-manifest release has no `files.json` to check against. That is
        // not a failure — it is an old archive — so the install proceeds, just
        // as `Verify File Integrity` falls back to the exe-exists check for the
        // same installs.
        super::VerifyOutcome::ManifestMissing => {
            tracing::info!("staged install has no files.json — skipping integrity check");
            Ok(())
        }
        other => {
            tracing::warn!(outcome = ?other, "staged install failed integrity check — aborting");
            Err(format!(
                "the downloaded files failed their integrity check ({}). \
                 Your existing install has not been changed — try the update again.",
                describe_verify_failure(&other)
            ))
        }
    }
}

/// One short human phrase for a failed staging verify, for the error the user
/// reads on the install prompt.
fn describe_verify_failure(outcome: &super::VerifyOutcome) -> String {
    match outcome {
        super::VerifyOutcome::FilesMissing { count, .. } => {
            format!("{count} file(s) missing from the download")
        }
        super::VerifyOutcome::FilesCorrupted { count, .. } => {
            format!("{count} file(s) did not match their checksum")
        }
        super::VerifyOutcome::ExecutableMissing { .. } => {
            "the game executable was not in the download".to_string()
        }
        super::VerifyOutcome::ManifestUnreadable(e) => format!("unreadable manifest: {e}"),
        super::VerifyOutcome::ManifestMissing => "no manifest".to_string(),
        super::VerifyOutcome::Ok { .. } => "ok".to_string(),
    }
}

/// Total bytes of regular files under `dir`, skipping any top-level entry named
/// `skip`. Blocking; called from `spawn_blocking`.
///
/// Errors are swallowed on purpose: this only feeds a progress percentage, and a
/// directory being rewritten underneath the walk (which is exactly what is
/// happening while it runs) must never fail an install.
fn dir_size_blocking(dir: &Path, skip: &str) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(skip) {
            continue;
        }
        // symlink_metadata, so a bundle's internal symlinks are counted as the
        // few bytes they are rather than double-counting their targets.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size_blocking(&path, skip);
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::branches::github::{GameRelease, ReleaseAsset};

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            download_url: format!("https://example.test/{name}"),
            // These tests only exercise asset *selection*; size is irrelevant
            // to which asset wins.
            size: 0,
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
