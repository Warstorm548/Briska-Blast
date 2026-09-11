//! Owned GitHub Releases list fetch.
//!
//! `self_update`'s `ReleaseList::fetch()` returns releases-or-an-opaque-error and
//! **throws away the HTTP status + headers**, so the rate-limit safety net can't
//! see `X-RateLimit-Reset` / `-Remaining` or tell a `403` rate-limit apart from a
//! network blip. This module is the minimal direct `reqwest` call that exposes
//! both, feeding `crate::ratelimit` (Part 3 of the rate-limit design doc).
//!
//! Footprint: fetches one page of up to `PER_PAGE` (=100, GitHub's max) releases,
//! still following `Link: rel="next"` for correctness if the repo ever exceeds
//! that. At the current 60 releases that's **one request per check** instead of
//! two.
//!
//! This module owns the *transport*; [`super::release_cache`] owns the sharing.
//! Together they close out Part 4 of the design doc: the cache collapses the boot
//! fan-out to a single call (fetch-once) and passes the stored `ETag` in here so
//! an unchanged repo answers `304` with no body.
//!
//! **A `304` still costs a request here.** GitHub exempts conditional requests
//! from the rate limit only for *authenticated* callers; we are unauthenticated
//! (a public client cannot ship a token), and measurement against the live API
//! confirms `x-ratelimit-used` increments on an unauthenticated `304`. So the
//! `ETag` buys **bandwidth** — ~363 KB of JSON down to an empty body — not
//! budget. The budget saving comes entirely from fetch-once.
//!
//! `self_update` is still used elsewhere for the launcher's binary self-update
//! swap and stale-artifact cleanup; only the *list fetch* is taken in-house.

use crate::ratelimit::{self, Gate};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// GitHub's maximum page size for the releases endpoint. One page covers the
/// whole repo today (46 releases as of 2026-06-16); the `Link: rel="next"` loop
/// below still handles a future overflow past 100.
const PER_PAGE: u32 = 100;
const USER_AGENT: &str = "briskablast-launcher";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A releases page is small JSON; this trips only on a genuinely hung connection.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One GitHub Release, trimmed to the fields the discovery code reads. Unknown
/// JSON fields are ignored by serde, so the full API payload deserializes fine.
///
/// `Serialize` is what lets [`super::release_cache`] persist the list to disk:
/// the trimmed form is ~38 KB against the 363 KB the API actually returns, so the
/// cache stores only what the launcher can read back.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Release {
    /// The git tag string, e.g. `game-v0.2.0-dev.1` / `launcher-v0.14.0`.
    pub tag_name: String,
    /// Markdown release notes. May be absent/null.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Asset {
    pub name: String,
    /// The GitHub REST API asset endpoint
    /// (`api.github.com/.../releases/assets/<id>`). This is the URL the installer
    /// downloads from with `Accept: application/octet-stream`; it 302-redirects to
    /// the CDN. Counts as one core-API request. (Matches what `self_update`'s own
    /// parser handed us — see installer.rs's asset-download comment.)
    pub url: String,
    /// Asset size in bytes, as reported by the API. Feeds the download step's
    /// weight in `updater::plan`, so the progress bar is proportioned from the
    /// real release rather than a constant.
    ///
    /// `default` is load-bearing twice over: the field is absent from every
    /// `releases-cache.json` written before it existed (a cold start must not
    /// have to re-fetch 363 KB just to learn sizes), and a hypothetical API
    /// response without it must not fail the whole parse. `0` is handled — the
    /// plan degrades to evenly-weighted steps.
    #[serde(default)]
    pub size: u64,
}

/// Why a fetch did not return releases.
#[derive(Debug)]
pub enum FetchError {
    /// Rate-limited — either the local gate was already closed, or this fetch hit
    /// a confirmed `403`/`429`. `resume_at` is epoch seconds.
    RateLimited { resume_at: i64 },
    /// Anything else: network/timeout, a non-rate-limit HTTP status, JSON parse.
    /// Never arms a cooldown.
    Other(String),
}

impl FetchError {
    /// A user-facing one-liner for the existing `Result<_, String>` surfaces.
    pub fn to_user_string(&self) -> String {
        match self {
            FetchError::RateLimited { resume_at } => format!(
                "GitHub rate limit reached \u{2014} checks resume at {}.",
                ratelimit::format_resume(*resume_at)
            ),
            FetchError::Other(s) => s.clone(),
        }
    }
}

/// Result of a conditional release-list fetch.
#[derive(Debug)]
pub enum FetchOutcome {
    /// The list changed (or we had no `ETag` to offer). Carries the full list and
    /// the new `ETag` to store for next time, if GitHub sent one.
    Fresh {
        releases: Vec<Release>,
        etag: Option<String>,
    },
    /// GitHub answered `304 Not Modified` — the caller's cached list is still
    /// current, and no body was transferred. Still counts as one request against
    /// the unauthenticated hourly limit (see the module docs).
    NotModified,
}

/// List every release for `owner/repo`, paginating at [`PER_PAGE`]/page.
/// Consults the rate-limit gate before sending (returns `RateLimited` without a
/// request when blocked), records the rate-limit headers off every response
/// (Layer B), and arms a cooldown on a confirmed `403`/`429` (Layer A).
///
/// When `if_none_match` is `Some`, the **first page only** is requested
/// conditionally. That restriction is deliberate: an `ETag` identifies one page,
/// and GitHub returns releases newest-first, so any newly published release
/// necessarily changes page 1. Combined with only storing a validator for
/// single-page results, a `304` therefore proves the whole cached list is
/// unchanged, while a `200` means we must re-read every page anyway.
pub async fn fetch_releases(
    owner: &str,
    repo: &str,
    if_none_match: Option<&str>,
) -> Result<FetchOutcome, FetchError> {
    // Gate first — a local file read, never a GitHub request.
    if let Gate::Blocked { resume_at } = ratelimit::gate() {
        tracing::info!(resume_at, "rate-limit gate closed; skipping GitHub release fetch");
        return Err(FetchError::RateLimited { resume_at });
    }

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| FetchError::Other(format!("http client build: {e}")))?;

    let mut url =
        format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page={PER_PAGE}");
    let mut all: Vec<Release> = Vec::new();
    let mut fresh_etag: Option<String> = None;
    let mut first_page = true;

    loop {
        let mut req = client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json");
        if first_page {
            if let Some(etag) = if_none_match {
                req = req.header(reqwest::header::IF_NONE_MATCH, etag);
            }
        }
        let resp = req
            .send()
            .await
            // A transport error means we never got an HTTP response — treat as a
            // plain failure, NEVER a rate-limit cooldown (the guardrail).
            .map_err(|e| FetchError::Other(format!("release list fetch: {e}")))?;

        let status = resp.status();
        let next = parse_link_next(resp.headers());

        match inspect(status, resp.headers()) {
            RateSignal::Limited { reset } => {
                let resume_at = ratelimit::note_rate_limited(reset);
                tracing::warn!(%status, ?reset, resume_at, "GitHub rate-limited; arming back-off");
                return Err(FetchError::RateLimited { resume_at });
            }
            // Record the budget so Layer B can pre-empt the next call.
            RateSignal::Healthy { remaining, reset } => ratelimit::note_response(remaining, reset),
        }

        // Checked before `is_success` — a 304 is NOT a success status, so letting
        // it fall through would report "nothing changed" as an HTTP error. Only
        // reachable on the first page (we only send the header there), and only
        // for a single-page list (see the ETag capture below), so the caller's
        // whole cached list is still current.
        if status == reqwest::StatusCode::NOT_MODIFIED {
            tracing::debug!("GitHub release list unchanged (304) — no body transferred");
            return Ok(FetchOutcome::NotModified);
        }

        if !status.is_success() {
            return Err(FetchError::Other(format!(
                "GitHub returned HTTP {} for {url}",
                status.as_u16()
            )));
        }

        if first_page {
            // Keep the validator ONLY for a single-page result. It validates
            // page 1 alone, but the snapshot we cache is every page joined —
            // and deleting or editing an older release leaves page 1 byte-identical
            // while pages 2+ change. A later `304` would then "prove" the aggregate
            // was current when it was not. Storing `None` for a multi-page list
            // makes the next fetch unconditional, which is always correct.
            fresh_etag = if next.is_none() {
                parse_etag(resp.headers())
            } else {
                None
            };
        }

        let page: Vec<Release> = resp
            .json()
            .await
            .map_err(|e| FetchError::Other(format!("release list parse: {e}")))?;
        all.extend(page);

        first_page = false;
        match next {
            Some(n) => url = n,
            None => break,
        }
    }

    // Past page 1 the stored ETag would no longer describe what we hold, so a
    // multi-page result deliberately keeps only page 1's validator — which is
    // exactly the one the next conditional request will send.
    Ok(FetchOutcome::Fresh {
        releases: all,
        etag: fresh_etag,
    })
}

/// One GitHub API response classified for the rate-limit net.
pub(crate) enum RateSignal {
    /// Confirmed `403`/`429` rate-limit. `reset` is epoch secs if the header was
    /// present (absent on the near-dead no-header path).
    Limited { reset: Option<i64> },
    /// Not rate-limited; carries the last-known budget for Layer B recording.
    Healthy {
        remaining: Option<u32>,
        reset: Option<i64>,
    },
}

/// Inspect a response's status + rate-limit headers. The single source of truth
/// for the confirmed-rate-limit guardrail, shared by `fetch_releases` and the
/// installer's asset download.
pub(crate) fn inspect(status: reqwest::StatusCode, headers: &HeaderMap) -> RateSignal {
    let remaining = parse_header::<u32>(headers, "x-ratelimit-remaining");
    let reset = parse_header::<i64>(headers, "x-ratelimit-reset");
    let has_retry_after = headers.contains_key(reqwest::header::RETRY_AFTER);
    if is_rate_limited(status, remaining, has_retry_after) {
        RateSignal::Limited { reset }
    } else {
        RateSignal::Healthy { remaining, reset }
    }
}

/// Confirmed-rate-limit classifier — the correctness guardrail. Only a `403`/`429`
/// **with** an explicit exhausted-budget signal (`X-RateLimit-Remaining: 0`) or a
/// `Retry-After` (secondary limit) counts. A bare `403`/`429` with neither, and
/// every transport error, is treated as an ordinary failure with no cooldown.
fn is_rate_limited(
    status: reqwest::StatusCode,
    remaining: Option<u32>,
    has_retry_after: bool,
) -> bool {
    let code = status.as_u16();
    (code == 403 || code == 429) && (remaining == Some(0) || has_retry_after)
}

/// Parse a header value as `T` (trimmed). `None` if absent or unparseable.
fn parse_header<T: std::str::FromStr>(headers: &HeaderMap, name: &str) -> Option<T> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Copy the response `ETag` verbatim. GitHub's releases endpoint returns a **weak**
/// validator (`W/"…"`); `If-None-Match` must echo the received value byte-for-byte,
/// so this deliberately does no unquoting or `W/` stripping.
fn parse_etag(headers: &HeaderMap) -> Option<String> {
    Some(headers.get(reqwest::header::ETAG)?.to_str().ok()?.to_string())
}

/// Extract the `rel="next"` URL from a GitHub `Link` header, if present.
fn parse_link_next(headers: &HeaderMap) -> Option<String> {
    let link = headers.get(reqwest::header::LINK)?.to_str().ok()?;
    for entry in link.split(',') {
        let (url_part, params) = entry.trim().split_once(';')?;
        if params.contains("rel=\"next\"") {
            let url = url_part.trim().trim_start_matches('<').trim_end_matches('>');
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use reqwest::StatusCode;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            let name = HeaderName::from_bytes(k.as_bytes()).unwrap();
            h.insert(name, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn parses_rate_limit_headers() {
        let h = headers(&[
            ("x-ratelimit-remaining", "37"),
            ("x-ratelimit-reset", "1700000000"),
        ]);
        assert_eq!(parse_header::<u32>(&h, "x-ratelimit-remaining"), Some(37));
        assert_eq!(parse_header::<i64>(&h, "x-ratelimit-reset"), Some(1_700_000_000));
        assert_eq!(parse_header::<u32>(&h, "x-ratelimit-used"), None);
    }

    /// The validator must survive a round-trip verbatim: GitHub's releases
    /// endpoint sends a weak ETag, and stripping the `W/` or the quotes would
    /// make the next `If-None-Match` fail to match, silently costing a request.
    #[test]
    fn etag_is_copied_verbatim() {
        let weak = headers(&[("etag", "W/\"63b5d2306cb3c2a1fd3f17cd9dac28b9\"")]);
        assert_eq!(
            parse_etag(&weak).as_deref(),
            Some("W/\"63b5d2306cb3c2a1fd3f17cd9dac28b9\"")
        );
        // Strong validators pass through untouched too.
        let strong = headers(&[("etag", "\"abc123\"")]);
        assert_eq!(parse_etag(&strong).as_deref(), Some("\"abc123\""));
        // Absent header -> nothing to store; the next fetch is unconditional.
        assert_eq!(parse_etag(&HeaderMap::new()), None);
    }

    #[test]
    fn extracts_link_next() {
        let h = headers(&[(
            "link",
            "<https://api.github.com/repositories/1/releases?per_page=30&page=2>; rel=\"next\", \
             <https://api.github.com/repositories/1/releases?per_page=30&page=4>; rel=\"last\"",
        )]);
        assert_eq!(
            parse_link_next(&h).as_deref(),
            Some("https://api.github.com/repositories/1/releases?per_page=30&page=2")
        );
        // Last page: only rel="prev"/"first" -> no next.
        let last = headers(&[(
            "link",
            "<https://api.github.com/repositories/1/releases?per_page=30&page=3>; rel=\"prev\"",
        )]);
        assert_eq!(parse_link_next(&last), None);
        // No Link header at all.
        assert_eq!(parse_link_next(&HeaderMap::new()), None);
    }

    /// The guardrail table: only a 403/429 with an exhausted-budget signal counts.
    #[test]
    fn rate_limit_classification() {
        // Healthy 200 — never rate-limited.
        assert!(!is_rate_limited(StatusCode::OK, Some(40), false));
        // Primary limit: 403 + remaining 0.
        assert!(is_rate_limited(StatusCode::FORBIDDEN, Some(0), false));
        // 429 + remaining 0.
        assert!(is_rate_limited(StatusCode::TOO_MANY_REQUESTS, Some(0), false));
        // Secondary limit: 403 + Retry-After, no remaining header.
        assert!(is_rate_limited(StatusCode::FORBIDDEN, None, true));
        // A 403 with neither signal (e.g. a genuine auth/permission 403) must NOT
        // arm a cooldown.
        assert!(!is_rate_limited(StatusCode::FORBIDDEN, None, false));
        // Remaining still above 0 on a 403 -> not a rate limit.
        assert!(!is_rate_limited(StatusCode::FORBIDDEN, Some(7), false));
        // A 404 is just an error, never a rate limit.
        assert!(!is_rate_limited(StatusCode::NOT_FOUND, Some(0), false));
    }
}
