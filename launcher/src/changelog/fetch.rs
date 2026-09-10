//! Conditional fetch + on-disk cache for the raw changelog files.
//!
//! These requests go to `raw.githubusercontent.com`, which is a plain file CDN
//! rather than the REST API: responses carry no `x-ratelimit-*` headers and do
//! not spend the 60/hour core budget. So unlike `updater::release_cache` this is
//! deliberately not gated by `crate::ratelimit` — gating it would make the
//! changelog stale for no benefit.
//!
//! `ETag` is still honoured, because it is free and turns a repeat fetch of an
//! unchanged file into a `304` with no body (verified against both files).

use super::Kind;
use crate::paths;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

const USER_AGENT: &str = "briskablast-launcher";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Each file is well under 100 KB; this trips only on a hung connection.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Sidecar mapping file name → last seen `ETag`.
const ETAGS_FILE: &str = "etags.json";

/// Result of a conditional changelog fetch.
pub enum Outcome {
    Fresh {
        text: String,
        etag: Option<String>,
    },
    /// The cached copy is still current; nothing was transferred.
    NotModified,
}

/// Process-wide client, mirroring `crate::server_api::http`'s lazy-init idiom so
/// repeat fetches reuse the connection pool.
fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builder")
    })
}

/// GET `url`, sending `If-None-Match` when an `ETag` is known.
pub async fn get(url: &str, etag: Option<&str>) -> Result<Outcome, String> {
    let mut req = http().get(url);
    if let Some(tag) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("changelog fetch: {e}"))?;

    let status = resp.status();
    // Checked before `is_success` — 304 is not a success status, and it is the
    // outcome we most want to recognise.
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Outcome::NotModified);
    }
    if !status.is_success() {
        return Err(format!("changelog fetch: HTTP {}", status.as_u16()));
    }

    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let text = resp
        .text()
        .await
        .map_err(|e| format!("changelog body: {e}"))?;
    Ok(Outcome::Fresh { text, etag })
}

/// Last `ETag` recorded for `kind`, if any. Any read/parse failure yields
/// `None`, which just costs one unconditional fetch.
pub fn read_etag(kind: Kind) -> Option<String> {
    read_etags().remove(kind.file_name())
}

/// Store the refreshed text and its validator. Best-effort: a write failure is
/// logged and ignored, costing only a re-fetch next launch.
pub fn persist(kind: Kind, text: &str, etag: Option<&str>) {
    let dir = match paths::changelog_dir_created() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "changelog dir unavailable; skipping persist");
            return;
        }
    };
    if let Err(e) = paths::write_atomic(&dir.join(kind.file_name()), text.as_bytes()) {
        tracing::warn!(error = %e, file = kind.file_name(), "failed to cache changelog");
        return;
    }
    // Only record the validator once the body it describes is safely on disk,
    // so a half-written cache can never be revalidated as if it were complete.
    let Some(tag) = etag else { return };
    let mut etags = read_etags();
    etags.insert(kind.file_name().to_string(), tag.to_string());
    match serde_json::to_vec_pretty(&etags) {
        Ok(bytes) => {
            if let Err(e) = paths::write_atomic(&dir.join(ETAGS_FILE), &bytes) {
                tracing::warn!(error = %e, "failed to write changelog etags (non-fatal)");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialize changelog etags"),
    }
}

fn read_etags() -> BTreeMap<String, String> {
    let Ok(dir) = paths::changelog_dir() else {
        return BTreeMap::new();
    };
    std::fs::read(dir.join(ETAGS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
