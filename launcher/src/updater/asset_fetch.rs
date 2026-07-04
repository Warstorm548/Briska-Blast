//! Release-asset fetch for the launcher's non-`self_update` swap paths
//! (macOS whole-bundle, Linux AppImage).
//!
//! `self_update` only knows how to swap a bare binary matched by target
//! triple; the bundle/AppImage paths instead need "the asset with *this name
//! suffix* from the `launcher-v<version>` release, streamed to *this* path".
//! Discovery reuses `github_client::fetch_releases` and the download mirrors
//! the game installer's pattern (`branches/installer/download.rs`): explicit
//! `Accept: application/octet-stream` (the API returns JSON metadata without
//! it), the rate-limit gate + `inspect` on the response, and a magic-byte
//! check so a truncated download or an error page never reaches the swap.

use super::github_client;
use futures_util::StreamExt;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Max time to establish a TCP+TLS connection to GitHub's asset CDN.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Max total time for one download. The launcher artifacts are small
/// (single-digit MB tar.gz / tens-of-MB AppImage); trips only on a hang.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// gzip magic — the macOS `.app` tarball.
#[cfg(target_os = "macos")]
pub(super) const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b];
/// ELF magic — an AppImage is an ELF with an appended squashfs.
#[cfg(target_os = "linux")]
pub(super) const ELF_MAGIC: &[u8] = &[0x7f, b'E', b'L', b'F'];

/// One asset resolved off a `launcher-v<version>` release.
pub(super) struct ReleaseAsset {
    pub name: String,
    /// GitHub REST API asset endpoint; 302-redirects to the CDN when
    /// requested with `Accept: application/octet-stream`.
    pub url: String,
}

/// Find the asset whose name ends with `suffix` on the `launcher-v<version>`
/// release. Goes through `github_client` so the rate-limit safety net sees
/// the request.
pub(super) async fn find_release_asset(
    version: &str,
    suffix: &str,
) -> Result<ReleaseAsset, String> {
    let tag = format!("{}{version}", super::github::TAG_PREFIX);
    let releases = github_client::fetch_releases(
        super::github::REPO_OWNER,
        super::github::REPO_NAME,
    )
    .await
    .map_err(|e| e.to_user_string())?;

    let release = releases
        .iter()
        .find(|r| r.tag_name == tag)
        .ok_or_else(|| format!("release {tag} not found on GitHub"))?;

    let asset = pick_asset(&release.assets, suffix)
        .ok_or_else(|| format!("release {tag} has no asset ending in {suffix}"))?;
    Ok(ReleaseAsset {
        name: asset.name.clone(),
        url: asset.url.clone(),
    })
}

/// `ends_with` rather than `contains` so a companion file such as
/// `…-macos-app.tar.gz.sha256` is never picked over the real archive.
fn pick_asset<'a>(
    assets: &'a [github_client::Asset],
    suffix: &str,
) -> Option<&'a github_client::Asset> {
    assets.iter().find(|a| a.name.ends_with(suffix))
}

/// Stream `asset` to `dest`, then verify the file starts with
/// `expected_magic`. On any error the (partial) `dest` file is left for the
/// caller's staging-dir cleanup.
pub(super) async fn download_to_file(
    asset: &ReleaseAsset,
    dest: &Path,
    expected_magic: &[u8],
) -> Result<(), String> {
    // Rate-limit gate: the asset endpoint is a counted core-API request.
    if let crate::ratelimit::Gate::Blocked { resume_at } = crate::ratelimit::gate() {
        return Err(format!(
            "GitHub rate limit reached \u{2014} update resumes at {}.",
            crate::ratelimit::format_resume(resume_at)
        ));
    }

    let client = reqwest::Client::builder()
        .user_agent("briskablast-launcher")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| format!("http client build: {e}"))?;

    let resp = client
        .get(&asset.url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("download request: {e}"))?;
    // A rate-limit is a direct 403/429 from api.github.com; a success is a
    // CDN 302 reqwest already followed (no GitHub budget headers to record).
    if let github_client::RateSignal::Limited { reset } =
        github_client::inspect(resp.status(), resp.headers())
    {
        let resume_at = crate::ratelimit::note_rate_limited(reset);
        return Err(format!(
            "GitHub rate limit reached \u{2014} update resumes at {}.",
            crate::ratelimit::format_resume(resume_at)
        ));
    }
    let resp = resp
        .error_for_status()
        .map_err(|e| format!("download HTTP error: {e}"))?;

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("create download file: {e}"))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("download chunk: {e}"))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("write chunk: {e}"))?;
        downloaded += bytes.len() as u64;
    }
    // Fully close before re-opening for the magic check (mirrors the game
    // installer's flush/sync/shutdown sequence).
    file.flush().await.map_err(|e| format!("flush download: {e}"))?;
    file.sync_all()
        .await
        .map_err(|e| format!("sync download: {e}"))?;
    file.shutdown()
        .await
        .map_err(|e| format!("close download: {e}"))?;
    drop(file);

    let mut head = vec![0u8; expected_magic.len()];
    {
        use tokio::io::AsyncReadExt;
        let mut f = tokio::fs::File::open(dest)
            .await
            .map_err(|e| format!("open download for magic check: {e}"))?;
        let n = f
            .read(&mut head)
            .await
            .map_err(|e| format!("read download magic: {e}"))?;
        if n < expected_magic.len() || head != expected_magic {
            return Err(format!(
                "downloaded {} is not the expected file type \
                 (first bytes: {:02x?}, size: {downloaded}) — likely an error \
                 response instead of the real artifact",
                asset.name,
                &head[..n]
            ));
        }
    }

    tracing::info!(
        asset = %asset.name,
        downloaded,
        dest = %dest.display(),
        "release asset downloaded, magic bytes ok"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> github_client::Asset {
        github_client::Asset {
            name: name.to_string(),
            url: format!("https://example.test/{name}"),
        }
    }

    /// The real archive must win over a checksum/signature companion whose
    /// name also *contains* the suffix (the `ends_with` guard).
    #[test]
    fn pick_asset_ignores_companion_files() {
        let assets = vec![
            asset("briskablast-launcher-0.20.0-macos-app.tar.gz.sha256"),
            asset("briskablast-launcher-0.20.0-macos-app.tar.gz"),
            asset("briskablast-launcher-0.20.0-linux.AppImage.sig"),
            asset("briskablast-launcher-0.20.0-linux.AppImage"),
            asset("briskablast-launcher-0.20.0-aarch64-apple-darwin.tar.gz"),
        ];
        let app = pick_asset(&assets, "-macos-app.tar.gz").unwrap();
        assert_eq!(app.name, "briskablast-launcher-0.20.0-macos-app.tar.gz");
        let ai = pick_asset(&assets, "-linux.AppImage").unwrap();
        assert_eq!(ai.name, "briskablast-launcher-0.20.0-linux.AppImage");
        assert!(pick_asset(&assets, "-windows.zip").is_none());
    }
}
