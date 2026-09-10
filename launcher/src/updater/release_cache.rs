//! Shared, conditional GitHub Releases list — the "fetch-once + ETag" half of
//! Part 4 in `docs/planning/launcher-github-ratelimit-safety-net.md`.
//!
//! Every GitHub Releases consumer in the launcher wants the **same** list off the
//! **same** repo: the self-update check, each visible channel's `latest_release`,
//! Repair's fetch-by-version, and the macOS/Linux self-update asset lookup. Before
//! this module they each issued their own request, so a returning user's boot cost
//! 3 requests (Stable + EA) or 4 (Dev flagged) out of GitHub's 60/hour/IP budget —
//! and every one of them pulled an identical 363 KB payload.
//!
//! Two mechanisms, stacked:
//!
//! * **Fetch-once.** A process-wide snapshot behind a `tokio` mutex. The lock is
//!   held *across the network call* on purpose — that is what makes the concurrent
//!   boot fan-out collapse into a single request. Callers queue, the first spends
//!   the request, the rest read the snapshot it just stored.
//! * **Conditional revalidation.** The snapshot (and its `ETag`) is persisted to
//!   `<data_dir>/releases-cache.json`, so a *cold* process starts warm and its very
//!   first request already carries `If-None-Match`. GitHub answers `304` with no
//!   body when nothing changed.
//!
//! Net effect on the budget: a returning user's boot goes from **3 counted requests
//! (Stable + EA) or 4 (Dev flagged) down to 1**. That saving comes from fetch-once.
//!
//! **The `ETag` saves bandwidth, not budget.** GitHub only exempts conditional
//! requests from the rate limit for *authenticated* callers; a public client cannot
//! ship a token, and measurement against the live API confirms `x-ratelimit-used`
//! increments on an unauthenticated `304`. What it does buy is ~363 KB of JSON per
//! revalidation collapsing to an empty body, which is still worth having.
//!
//! Fails soft in the same spirit as `crate::ratelimit`: a missing or corrupt cache
//! file just means "no snapshot", never a broken update check.

use super::github_client::{self, FetchError, FetchOutcome, Release};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

/// How long an in-memory snapshot satisfies a [`Freshness::Cached`] request with
/// **no** network call at all. Sized to cover one boot's fan-out plus an immediate
/// relaunch, not to keep data warm for a session — anything longer risks showing a
/// version that changed under us.
const MEM_TTL_SECS: i64 = 60;

/// How current a caller needs the release list to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Serve an in-memory snapshot younger than [`MEM_TTL_SECS`] without any
    /// request. Used by the boot fan-out and the install prompt, where several
    /// callers want the same list within milliseconds of each other.
    Cached,
    /// Always send a conditional request, even with a warm snapshot. Used where a
    /// stale answer would be wrong: a manual update check (the user explicitly
    /// asked), and anything that precedes a download (so an install can never be
    /// aimed at a release that has moved). This always spends a request — the
    /// conditional form only avoids re-downloading the body.
    Revalidate,
}

/// One fetched view of the release list, plus the validator that lets the next
/// request revalidate it for free.
#[derive(Debug, Clone)]
struct Snapshot {
    /// `Arc` so handing the list to callers never clones ~60 releases.
    releases: Arc<Vec<Release>>,
    /// GitHub's `ETag` for page 1, echoed verbatim on the next `If-None-Match`.
    etag: Option<String>,
    /// Epoch seconds this snapshot was last confirmed current.
    fetched_at: i64,
}

/// On-disk form of [`Snapshot`], written to `<data_dir>/releases-cache.json`.
/// Only the trimmed fields `Release` declares are stored (~38 KB against the
/// 363 KB the API returns).
#[derive(Debug, Serialize, Deserialize)]
struct DiskCache {
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    fetched_at: i64,
    releases: Vec<Release>,
}

#[derive(Default)]
struct CacheState {
    snapshot: Option<Snapshot>,
    /// The disk read is attempted exactly once per process, on first use. A
    /// missing file is a normal first-run state, so "tried and found nothing" has
    /// to be distinguishable from "not tried yet".
    disk_loaded: bool,
}

/// Process-wide cache. `OnceLock` + `tokio::sync::Mutex` mirrors the lazy-client
/// idiom already used by `crate::server_api::http`.
fn cache() -> &'static Mutex<CacheState> {
    static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CacheState::default()))
}

/// The full release list for `owner/repo`, shared across every caller.
///
/// See [`Freshness`] for the two request modes. Errors match the previous direct
/// `fetch_releases` behavior, except that a `Cached` caller with a usable snapshot
/// is served that snapshot instead of failing (see [`should_serve_stale`]).
pub async fn releases(
    owner: &str,
    repo: &str,
    freshness: Freshness,
) -> Result<Arc<Vec<Release>>, FetchError> {
    // Held across the await deliberately — see the module docs. This is the
    // single-flight: it is why the boot fan-out costs one request, not four.
    let mut state = cache().lock().await;

    if !state.disk_loaded {
        state.snapshot = load_from_disk();
        state.disk_loaded = true;
    }

    let now = now_secs();
    if let Some(snap) = state.snapshot.as_ref() {
        if serves_from_memory(freshness, snap.fetched_at, now) {
            tracing::debug!("serving release list from the in-memory snapshot (no request)");
            return Ok(Arc::clone(&snap.releases));
        }
    }

    let etag = state.snapshot.as_ref().and_then(|s| s.etag.clone());
    match github_client::fetch_releases(owner, repo, etag.as_deref()).await {
        Ok(FetchOutcome::NotModified) => {
            // Only reachable when we sent an ETag, which we only do when a
            // snapshot exists — so the `else` here is an unreachable-in-practice
            // guard rather than a real state.
            let Some(snap) = state.snapshot.as_mut() else {
                return Err(FetchError::Other(
                    "GitHub returned 304 with no cached release list".to_string(),
                ));
            };
            // In-memory only: the bytes on disk are unchanged, so rewriting 38 KB
            // just to bump a timestamp would be waste. The stale on-disk
            // `fetched_at` is conservative — worst case the next cold start
            // revalidates, which is itself free.
            snap.fetched_at = now;
            Ok(Arc::clone(&snap.releases))
        }
        Ok(FetchOutcome::Fresh { releases, etag }) => {
            tracing::debug!(count = releases.len(), "release list refreshed from GitHub");
            let snap = Snapshot {
                releases: Arc::new(releases),
                etag,
                fetched_at: now,
            };
            persist(&snap);
            let out = Arc::clone(&snap.releases);
            state.snapshot = Some(snap);
            Ok(out)
        }
        Err(e) => {
            let snapshot = state.snapshot.as_ref();
            if should_serve_stale(freshness, snapshot.is_some()) {
                // Unwrap-free: should_serve_stale only returns true when present.
                if let Some(snap) = snapshot {
                    tracing::warn!(
                        error = %e.to_user_string(),
                        "release list fetch failed; serving the cached list"
                    );
                    return Ok(Arc::clone(&snap.releases));
                }
            }
            Err(e)
        }
    }
}

// ---- pure decision helpers (unit-tested) ----

/// Whether an existing snapshot can answer without any request. Only
/// [`Freshness::Cached`] may short-circuit, and only inside the TTL.
fn serves_from_memory(freshness: Freshness, fetched_at: i64, now: i64) -> bool {
    freshness == Freshness::Cached && now.saturating_sub(fetched_at) < MEM_TTL_SECS
}

/// Whether a failed fetch should fall back to the cached list instead of
/// surfacing the error ("stale-if-error").
///
/// Deliberately limited to [`Freshness::Cached`]. On boot, showing the last known
/// versions beats blanking the UI over a Wi-Fi blip. But a `Revalidate` caller
/// either asked a direct question ("is there an update?") or is about to download
/// something, and both of those must hear the truth — this is what preserves the
/// existing "⏳ GitHub limit — retry at HH:MM" verdict on a manual check.
fn should_serve_stale(freshness: Freshness, has_snapshot: bool) -> bool {
    has_snapshot && freshness == Freshness::Cached
}

// ---- persistence ----

/// Read the persisted snapshot. Any failure (missing, unreadable, corrupt) yields
/// `None` so a bad file degrades to "no cache" rather than bricking updates.
fn load_from_disk() -> Option<Snapshot> {
    let path = paths::releases_cache_path().ok()?;
    let bytes = std::fs::read(&path).ok()?;
    let disk: DiskCache = serde_json::from_slice(&bytes).ok()?;
    tracing::debug!(count = disk.releases.len(), "loaded release cache from disk");
    Some(Snapshot {
        releases: Arc::new(disk.releases),
        etag: disk.etag,
        fetched_at: disk.fetched_at,
    })
}

/// Persist a freshly fetched snapshot. Best-effort: a write failure is logged and
/// ignored, costing only a non-conditional request on the next cold start.
fn persist(snap: &Snapshot) {
    let path = match paths::releases_cache_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "releases cache path unavailable; skipping persist");
            return;
        }
    };
    let disk = DiskCache {
        etag: snap.etag.clone(),
        fetched_at: snap.fetched_at,
        releases: (*snap.releases).clone(),
    };
    // Compact, not pretty: this is a machine-read cache, and pretty-printing it
    // would inflate the 38 KB write for no one's benefit.
    match serde_json::to_vec(&disk) {
        Ok(bytes) => {
            if let Err(e) = paths::write_atomic(&path, &bytes) {
                tracing::warn!(error = %e, "failed to persist releases-cache.json (non-fatal)");
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to serialize releases cache (non-fatal)"),
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_serves_from_memory_inside_the_ttl() {
        let now = 1_000_000;
        assert!(serves_from_memory(Freshness::Cached, now, now));
        assert!(serves_from_memory(Freshness::Cached, now - MEM_TTL_SECS + 1, now));
    }

    #[test]
    fn cached_revalidates_past_the_ttl() {
        let now = 1_000_000;
        assert!(!serves_from_memory(Freshness::Cached, now - MEM_TTL_SECS, now));
        assert!(!serves_from_memory(Freshness::Cached, now - 3600, now));
    }

    /// A manual check must always reach GitHub, however warm the snapshot is —
    /// otherwise the button would report a verdict without checking anything.
    #[test]
    fn revalidate_never_serves_from_memory() {
        let now = 1_000_000;
        assert!(!serves_from_memory(Freshness::Revalidate, now, now));
    }

    /// A clock that jumped backwards must not wrap the age into a huge positive
    /// number (or panic in debug) — a future timestamp still counts as fresh.
    #[test]
    fn future_timestamp_does_not_underflow() {
        let now = 1_000_000;
        assert!(serves_from_memory(Freshness::Cached, now + 500, now));
    }

    #[test]
    fn stale_if_error_only_on_the_boot_path() {
        // Boot with something cached: show the last known list.
        assert!(should_serve_stale(Freshness::Cached, true));
        // Nothing cached: there is nothing to serve, so the error propagates.
        assert!(!should_serve_stale(Freshness::Cached, false));
        // Manual check / pre-download: the caller must hear the failure even
        // though a snapshot exists.
        assert!(!should_serve_stale(Freshness::Revalidate, true));
        assert!(!should_serve_stale(Freshness::Revalidate, false));
    }

    #[test]
    fn disk_cache_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("releases-cache.json");
        let disk = DiskCache {
            etag: Some("W/\"abc\"".to_string()),
            fetched_at: 1_700_000_000,
            releases: Vec::new(),
        };
        let bytes = serde_json::to_vec(&disk).unwrap();
        paths::write_atomic(&path, &bytes).unwrap();

        let read: DiskCache = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(read.etag.as_deref(), Some("W/\"abc\""));
        assert_eq!(read.fetched_at, 1_700_000_000);

        // The uuid-suffixed staging file must not linger after the rename.
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "tmp file should be renamed away");
    }

    /// A corrupt cache file must read as "no snapshot", never as an error that
    /// blocks update checks.
    #[test]
    fn corrupt_disk_cache_parses_to_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("releases-cache.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let parsed: Option<DiskCache> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        assert!(parsed.is_none());
    }
}
