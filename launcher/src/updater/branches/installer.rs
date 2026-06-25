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
use std::collections::BTreeMap;
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

pub const FILES_MANIFEST_FILENAME: &str = "files.json";

/// The only `files.json` schema version this launcher understands. CI writes
/// `"schema": 1`; `files_manifest` rejects anything else.
pub const FILES_MANIFEST_SCHEMA: u32 = 1;

/// Build-time per-file integrity manifest, generated in CI and shipped *inside*
/// the release archive as `files.json`. Read by `verify_install` to confirm
/// every shipped file is present, the right size, and (deep pass) the right
/// bytes. Distinct from `installed.json` (which the launcher writes post-extract
/// and records version/channel/exe). The CI generator emits `files.json` last,
/// so it never lists itself; `installed.json` and `saves/` are likewise absent
/// from the map and therefore ignored by verify.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesManifest {
    /// Schema version so a future shape change is detectable. Currently `1`.
    pub schema: u32,
    /// Relative path (forward-slash separated, relative to the install dir) →
    /// expected size + sha256.
    pub files: BTreeMap<String, FileEntry>,
}

/// One file's expected size and lowercase-hex SHA-256, as recorded at build time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub size: u64,
    pub sha256: String,
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
        tokio::fs::rename(&final_install_dir, &aside)
            .await
            .map_err(|e| format!("move old install aside: {e}"))?;
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

fn extract_archive_blocking(archive: &Path, dest: &Path, name: &str) -> Result<String, String> {
    use std::fs::File;
    if name.ends_with(".tar.gz") {
        let f = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest)
            .map_err(|e| format!("tar unpack: {e}"))?;
        // Linux ships a bare ELF (BriskaBlast.x86_64) at the archive top level;
        // macOS ships a BriskaBlast.app bundle whose Mach-O lives a few levels
        // deep (Contents/MacOS/). Both arrive as .tar.gz, so the executable
        // resolution — not the extraction — is what differs by platform.
        #[cfg(target_os = "macos")]
        {
            find_app_executable(dest).ok_or_else(|| {
                "extracted archive does not contain a *.app/Contents/MacOS/ executable".to_string()
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            find_executable(dest, "BriskaBlast.x86_64")
                .ok_or_else(|| "extracted archive does not contain BriskaBlast.x86_64".to_string())
        }
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

/// Locate the macOS game executable inside an extracted `.app` bundle. Finds the
/// single top-level `*.app`, then the Mach-O in its `Contents/MacOS/` (Godot
/// names it after the bundle, but we glob for the one file rather than hardcode
/// the name). Returns the path relative to `dir`, e.g.
/// `BriskaBlast.app/Contents/MacOS/BriskaBlast`, to store as the manifest
/// executable. Only called on macOS; the logic is OS-agnostic so the test below
/// exercises it on every platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn find_app_executable(dir: &Path) -> Option<String> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let app = entry.path();
        if app.is_dir() && app.extension().and_then(|e| e.to_str()) == Some("app") {
            let macos_dir = app.join("Contents").join("MacOS");
            for bin in std::fs::read_dir(&macos_dir).ok()?.flatten() {
                let p = bin.path();
                if p.is_file() {
                    return p
                        .strip_prefix(dir)
                        .ok()
                        .map(|r| r.to_string_lossy().to_string());
                }
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

/// Read `<install_dir>/files.json`. `Ok(None)` when absent — an older install
/// packaged before per-file manifests existed, in which case `verify_install`
/// falls back to the cheap exe-exists check. Parse errors bubble up so a
/// corrupted manifest is reported rather than silently treated as "no manifest".
pub async fn files_manifest(install_dir: &Path) -> Result<Option<FilesManifest>, String> {
    let path = install_dir.join(FILES_MANIFEST_FILENAME);
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => {
            let m: FilesManifest =
                serde_json::from_str(&s).map_err(|e| format!("files manifest parse: {e}"))?;
            // Compatibility gate: refuse a manifest written by a future,
            // incompatible schema rather than silently trusting fields whose
            // meaning may have changed.
            if m.schema != FILES_MANIFEST_SCHEMA {
                return Err(format!(
                    "files manifest schema {} is unsupported (this launcher understands {})",
                    m.schema, FILES_MANIFEST_SCHEMA
                ));
            }
            Ok(Some(m))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("files manifest read: {e}")),
    }
}

/// True only for a strictly-relative path with no root, drive prefix, or `..`
/// components — so joining it onto the install dir can never escape the tree.
/// Guards `verify_files` against a tampered `files.json` pointing it at files
/// outside the install (e.g. `../../etc/passwd` or `/etc/shadow`).
fn is_safe_relpath(rel: &str) -> bool {
    use std::path::Component;
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Subdirectory of the install_root (the parent of the install_dir) where
/// saves are moved when the user picks "Keep saves" during uninstall. Per
/// the Stage 7 design: backups are timestamped so a re-install / re-uninstall
/// cycle never overwrites prior saves.
pub const SAVES_BACKUP_DIRNAME: &str = ".briska-saves-backup";

/// Outcome of a Verify File Integrity check. The diagnostic payloads
/// (`ManifestUnreadable`'s string, `ExecutableMissing`'s path) are surfaced
/// via the `?outcome` Debug formatter on the VerifyComplete tracing log;
/// the inline status cell in Settings shows only a short label. Allow
/// dead_code so the fields can be promoted to a hover tooltip later
/// without rewriting the enum.
#[derive(Debug, Clone)]
pub enum VerifyOutcome {
    Ok {
        version: String,
    },
    ManifestMissing,
    ManifestUnreadable(#[allow(dead_code)] String),
    ExecutableMissing {
        #[allow(dead_code)]
        expected: PathBuf,
    },
    /// Deep verify (cheap pass): files listed in `files.json` that are absent or
    /// the wrong size on disk. `count` is the total; `sample` holds up to a few
    /// relpaths for the `?outcome` tracing log / a future hover tooltip.
    FilesMissing {
        count: usize,
        #[allow(dead_code)]
        sample: Vec<String>,
    },
    /// Deep verify (hash pass): files present and the right size, but whose
    /// sha256 doesn't match the manifest — corruption or tampering.
    FilesCorrupted {
        count: usize,
        #[allow(dead_code)]
        sample: Vec<String>,
    },
}

/// Integrity check. Reads `installed.json` for the version, then — when the
/// build shipped a `files.json` — verifies every listed file is present, the
/// right size (cheap pass), and the right bytes (deep sha256 pass on a blocking
/// thread, since the `.pck` is hundreds of MB). Falls back to the historic
/// exe-exists check when `files.json` is absent (installs packaged before
/// per-file manifests).
pub async fn verify_install(install_dir: PathBuf) -> VerifyOutcome {
    let manifest = match installed_manifest(&install_dir).await {
        Ok(Some(m)) => m,
        Ok(None) => return VerifyOutcome::ManifestMissing,
        Err(e) => return VerifyOutcome::ManifestUnreadable(e),
    };

    match files_manifest(&install_dir).await {
        Ok(Some(files)) => verify_files(&install_dir, &files, &manifest.version).await,
        Ok(None) => {
            // Legacy install (no files.json) — fall back to the cheap exe check.
            let exe = install_dir.join(&manifest.executable);
            match tokio::fs::metadata(&exe).await {
                Ok(_) => VerifyOutcome::Ok {
                    version: manifest.version,
                },
                Err(_) => VerifyOutcome::ExecutableMissing { expected: exe },
            }
        }
        Err(e) => VerifyOutcome::ManifestUnreadable(e),
    }
}

/// Two-pass per-file verify against `files.json`. Pass 1 (cheap, async): every
/// listed file is present and its size matches — catches the common breakage
/// (deleted / truncated / half-extracted files) instantly without reading bytes.
/// Pass 2 (deep): sha256 of each survivor matches, run on a blocking thread with
/// chunked reads so a multi-hundred-MB `.pck` doesn't stall the async runtime.
/// Files on disk that aren't in the manifest (e.g. `installed.json`, `saves/`)
/// are ignored — verify only asserts the *manifest's* files.
async fn verify_files(install_dir: &Path, files: &FilesManifest, version: &str) -> VerifyOutcome {
    /// How many failing relpaths to retain for diagnostics (the full count is
    /// always reported; the sample bounds the log/tooltip size).
    const SAMPLE_MAX: usize = 5;

    // Pass 1: presence + size.
    let mut missing: Vec<String> = Vec::new();
    for (rel, entry) in &files.files {
        // Reject unsafe entries (absolute / `..` / drive-prefix) before touching
        // the filesystem — a tampered manifest must not escape the install tree.
        if !is_safe_relpath(rel) {
            missing.push(rel.clone());
            continue;
        }
        let path = install_dir.join(rel);
        let ok = matches!(tokio::fs::metadata(&path).await, Ok(m) if m.len() == entry.size);
        if !ok {
            missing.push(rel.clone());
        }
    }
    if !missing.is_empty() {
        let count = missing.len();
        missing.truncate(SAMPLE_MAX);
        return VerifyOutcome::FilesMissing {
            count,
            sample: missing,
        };
    }

    // Pass 2: sha256. Offloaded to a blocking thread (chunked I/O over GBs).
    let dir = install_dir.to_path_buf();
    let files = files.clone();
    let corrupted = match tokio::task::spawn_blocking(move || {
        let mut bad: Vec<String> = Vec::new();
        for (rel, entry) in &files.files {
            // Same containment guard as pass 1, applied before hashing.
            if !is_safe_relpath(rel) {
                bad.push(rel.clone());
                continue;
            }
            let hashed = hash_file_blocking(&dir.join(rel));
            let ok = matches!(hashed, Ok(ref hex) if hex.eq_ignore_ascii_case(&entry.sha256));
            if !ok {
                bad.push(rel.clone());
            }
        }
        bad
    })
    .await
    {
        Ok(bad) => bad,
        Err(e) => {
            // A join failure means we couldn't confirm integrity — report it as
            // corruption rather than silently passing.
            tracing::warn!(error = %e, "verify hash task failed to join");
            vec!["<hash task failed>".to_string()]
        }
    };

    if corrupted.is_empty() {
        VerifyOutcome::Ok {
            version: version.to_string(),
        }
    } else {
        let count = corrupted.len();
        let mut sample = corrupted;
        sample.truncate(SAMPLE_MAX);
        VerifyOutcome::FilesCorrupted { count, sample }
    }
}

/// SHA-256 a file with a fixed-size buffer (never reads the whole file into
/// memory — the `.pck` can be ~1 GB). Returns lowercase hex. Blocking; call
/// only inside `spawn_blocking`.
fn hash_file_blocking(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Tear down a channel's installation. `install_dir` is the resolved
/// `<install_root>/<channel.dir_name()>/` recorded in identity.json.
///
/// `keep_saves` honours foundation §2's "Keep player data for future
/// reinstall?" prompt — when true, `<install_dir>/saves/` is moved to a
/// **timestamped** sibling under `<install_root>/.briska-saves-backup/
/// <channel.dir_name()>/<rfc3339>/` before the install dir is removed so
/// a subsequent reinstall + re-uninstall cycle never overwrites a prior
/// backup. When false the saves go with the install.
pub async fn uninstall_install(
    install_dir: PathBuf,
    channel_dir_name: &'static str,
    keep_saves: bool,
) -> Result<(), String> {
    if !install_dir.exists() {
        // Nothing on disk — treat as success so the caller can still
        // clear identity.json. The user did ask for "uninstalled", after all.
        return Ok(());
    }

    // Defence-in-depth: the launcher computes install_dir as
    // <user-picked install_root>/<channel.dir_name()>/, but
    // identity.json could be hand-edited to point install_location
    // somewhere else (e.g. /home/user, /etc). Before `remove_dir_all`
    // we canonicalize and require the final path component to match
    // the channel_dir_name passed in. Anything else returns Err — the
    // launcher will surface the message on the uninstall prompt
    // rather than wiping an unrelated directory.
    let canonical = tokio::fs::canonicalize(&install_dir).await.map_err(|e| {
        format!(
            "canonicalize install_dir {}: {e}",
            install_dir.display()
        )
    })?;
    let last = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            format!(
                "install_dir {} has no terminal component",
                canonical.display()
            )
        })?;
    if last != channel_dir_name {
        return Err(format!(
            "install_dir {} does not end in expected channel name {:?} — refusing to remove",
            canonical.display(),
            channel_dir_name
        ));
    }

    if keep_saves {
        let saves = install_dir.join("saves");
        if saves.exists() {
            let install_root = install_dir.parent().ok_or_else(|| {
                format!(
                    "install_dir {} has no parent — cannot place saves backup",
                    install_dir.display()
                )
            })?;
            // Millisecond precision avoids a collision when two
            // uninstalls land in the same calendar second (e.g. test
            // harnesses, rapid retries). The `%.3f` chrono token writes
            // `.NNN` with a literal dot — filesystem-safe on Linux,
            // macOS, and Windows (no `:`).
            let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
            let backup_dir = install_root
                .join(SAVES_BACKUP_DIRNAME)
                .join(channel_dir_name)
                .join(stamp);
            if let Some(parent) = backup_dir.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("create saves backup parent: {e}"))?;
            }
            tokio::fs::rename(&saves, &backup_dir).await.map_err(|e| {
                format!(
                    "move {} → {}: {e}",
                    saves.display(),
                    backup_dir.display()
                )
            })?;
            tracing::info!(
                from = %saves.display(),
                to = %backup_dir.display(),
                "saves backed up before uninstall"
            );
        }
    }

    tokio::fs::remove_dir_all(&install_dir)
        .await
        .map_err(|e| format!("remove install dir {}: {e}", install_dir.display()))?;
    Ok(())
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

    /// The in-bundle resolver returns the Mach-O path relative to the install
    /// dir, e.g. `BriskaBlast.app/Contents/MacOS/BriskaBlast`. OS-agnostic logic,
    /// so this runs on every platform's CI leg.
    #[test]
    fn find_app_executable_locates_in_bundle_macho() {
        let tmp = tempfile::tempdir().unwrap();
        let macos = tmp
            .path()
            .join("BriskaBlast.app")
            .join("Contents")
            .join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("BriskaBlast"), b"\x7fELF-or-macho").unwrap();

        let rel = find_app_executable(tmp.path()).expect("should find the in-bundle executable");
        assert_eq!(rel, "BriskaBlast.app/Contents/MacOS/BriskaBlast");
    }

    #[test]
    fn find_app_executable_none_without_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("BriskaBlast.x86_64"), b"elf").unwrap();
        assert!(find_app_executable(tmp.path()).is_none());
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

    /// Mirror of `hash_file_blocking` for computing the test's expected digests.
    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        format!("{:x}", h.finalize())
    }

    /// Deep verify against a `files.json`: a clean tree passes; a same-size byte
    /// change is caught by the hash pass; a deleted file is caught by the cheap
    /// presence/size pass (before any hashing). Exercises the whole
    /// `verify_install` → `verify_files` path on every platform.
    #[tokio::test]
    async fn verify_install_deep_pass_detects_corruption_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(dir.join("BriskaBlast.x86_64"), b"the-binary-bytes").unwrap();
        std::fs::write(dir.join("game.pck"), b"pack-contents").unwrap();

        let installed = InstalledManifest {
            version: "0.17.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-24T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();

        let mut files = BTreeMap::new();
        for name in ["BriskaBlast.x86_64", "game.pck"] {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            files.insert(
                name.to_string(),
                FileEntry {
                    size: bytes.len() as u64,
                    sha256: sha256_hex(&bytes),
                },
            );
        }
        std::fs::write(
            dir.join(FILES_MANIFEST_FILENAME),
            serde_json::to_vec(&FilesManifest { schema: 1, files }).unwrap(),
        )
        .unwrap();

        // Clean tree → Ok. (Extra unlisted files like installed.json are ignored.)
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::Ok { .. }
        ));

        // Same-length byte change → passes the size pass, fails the hash pass.
        std::fs::write(dir.join("game.pck"), b"pack-CONTENTS").unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesCorrupted { count: 1, .. }
        ));

        // Deleted file → caught by the cheap pass before any hashing.
        std::fs::remove_file(dir.join("game.pck")).unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesMissing { count: 1, .. }
        ));
    }

    /// No `files.json` (an install packaged before per-file manifests) falls
    /// back to the historic exe-exists check rather than erroring.
    #[tokio::test]
    async fn verify_install_without_files_manifest_falls_back_to_exe_check() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("BriskaBlast.x86_64"), b"bin").unwrap();
        let installed = InstalledManifest {
            version: "0.16.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-24T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::Ok { .. }
        ));

        std::fs::remove_file(dir.join("BriskaBlast.x86_64")).unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::ExecutableMissing { .. }
        ));
    }

    /// A files.json from a future, incompatible schema is rejected rather than
    /// trusted (compatibility gate).
    #[tokio::test]
    async fn files_manifest_rejects_unknown_schema() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(FILES_MANIFEST_FILENAME),
            br#"{"schema": 2, "files": {}}"#,
        )
        .unwrap();
        assert!(files_manifest(tmp.path()).await.is_err());
    }

    /// A manifest entry with a traversal path must be rejected (reported as a
    /// failure), never followed outside the install dir.
    #[tokio::test]
    async fn verify_rejects_path_traversal_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("BriskaBlast.x86_64"), b"bin").unwrap();
        let installed = InstalledManifest {
            version: "0.17.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-25T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();
        let mut files = BTreeMap::new();
        files.insert(
            "../escape.txt".to_string(),
            FileEntry {
                size: 1,
                sha256: "00".into(),
            },
        );
        std::fs::write(
            dir.join(FILES_MANIFEST_FILENAME),
            serde_json::to_vec(&FilesManifest { schema: 1, files }).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesMissing { count: 1, .. }
        ));
    }

    #[test]
    fn is_safe_relpath_rejects_escapes() {
        assert!(is_safe_relpath("a/b/c.dll"));
        assert!(is_safe_relpath("BriskaBlast.pck"));
        assert!(!is_safe_relpath("../escape"));
        assert!(!is_safe_relpath("a/../../b"));
        assert!(!is_safe_relpath("/abs/path"));
        assert!(!is_safe_relpath(""));
    }
}
