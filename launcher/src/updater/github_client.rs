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
//! that. At the current ~46 releases that's **one request per check** instead of
//! two. The remaining Part 4 reductions are still deferred: sharing a single fetch
//! across the self-update check + every channel (fetch-once), and ETag /
//! `If-None-Match` conditional requests (a `304` doesn't count against the limit).
//!
//! `self_update` is still used elsewhere for the launcher's binary self-update
//! swap and stale-artifact cleanup; only the *list fetch* is taken in-house.

use crate::ratelimit::{self, Gate};
use reqwest::header::HeaderMap;
use serde::Deserialize;
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
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    /// The git tag string, e.g. `game-v0.2.0-dev.1` / `launcher-v0.14.0`.
    pub tag_name: String,
    /// Markdown release notes. May be absent/null.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    /// The GitHub REST API asset endpoint
    /// (`api.github.com/.../releases/assets/<id>`). This is the URL the installer
    /// downloads from with `Accept: application/octet-stream`; it 302-redirects to
    /// the CDN. Counts as one core-API request. (Matches what `self_update`'s own
    /// parser handed us — see installer.rs's asset-download comment.)
    pub url: String,
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

/// List every release for `owner/repo`, paginating at 30/page. Consults the
/// rate-limit gate before sending (returns `RateLimited` without a request when
/// blocked), records the rate-limit headers off every response (Layer B), and
/// arms a cooldown on a confirmed `403`/`429` (Layer A).
pub async fn fetch_releases(owner: &str, repo: &str) -> Result<Vec<Release>, FetchError> {
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

    loop {
        let resp = client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
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

        if !status.is_success() {
            return Err(FetchError::Other(format!(
                "GitHub returned HTTP {} for {url}",
                status.as_u16()
            )));
        }

        let page: Vec<Release> = resp
            .json()
            .await
            .map_err(|e| FetchError::Other(format!("release list parse: {e}")))?;
        all.extend(page);

        match next {
            Some(n) => url = n,
            None => break,
        }
    }

    Ok(all)
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
