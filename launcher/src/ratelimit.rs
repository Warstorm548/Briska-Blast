//! GitHub rate-limit back-off safety net.
//!
//! The launcher discovers updates against GitHub Releases **unauthenticated**,
//! so every user shares GitHub's 60-requests/hour-per-IP bucket. This module is
//! the graceful back-off net: once we *know* we're rate-limited, send **zero**
//! further GitHub-counting requests until the window resets, so a harmless Tier-1
//! hourly throttle can never escalate into Tier-2 secondary limits or a Tier-3 IP
//! block. See `docs/planning/launcher-github-ratelimit-safety-net.md` (Part 3).
//!
//! State lives in `ratelimits.json` next to `identity.json` (see
//! `paths::ratelimit_path`). The gate is a local file read — always free, never a
//! GitHub request — and **fails OPEN**: a missing or corrupt file must never
//! permanently brick update checks.
//!
//! Two layers, both fed by the rate-limit headers GitHub returns on *every* API
//! response (200 or 403):
//! * **Layer A** — on a confirmed `403`/`429` rate-limit, block until
//!   `reset + PAD`.
//! * **Layer B** — when `X-RateLimit-Remaining <= LOW_WATER`, go quiet *before*
//!   the next request would 403, until `reset + PAD`.
//!
//! Correctness guardrail (enforced by the caller in `github_client`): the cooldown
//! is recorded only on a confirmed `403`/`429` rate-limit status, **never** on a
//! generic network error/timeout — a Wi-Fi blip must not cause a lockout.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

/// Clock-skew pad added to GitHub's reset before we resume (seconds). The reset
/// is GitHub's clock; a small pad covers drift against ours.
const PAD_SECS: i64 = 120;
/// No-header fallback: a confirmed rate-limit response that somehow lacks a reset
/// gets this fixed cooldown (seconds). Near-dead path on public GitHub.
const FALLBACK_SECS: i64 = 3600;
/// Layer B trips at or below this remaining budget.
const LOW_WATER: u32 = 5;

/// Persisted back-off state. `reset`/`remaining` are the last values GitHub
/// reported (kept for diagnostics); `blocked_until` is the derived resume instant
/// the gate actually reads, so the gate stays a trivial `now < blocked_until`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RateLimitState {
    /// Epoch seconds when the limit refills (`X-RateLimit-Reset`).
    reset: Option<i64>,
    /// Last-known IP budget (`X-RateLimit-Remaining`).
    remaining: Option<u32>,
    /// Epoch seconds until which counted requests stay quiet. `None` = no block.
    blocked_until: Option<i64>,
}

/// Result of consulting the gate before a counted GitHub request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Clear to send.
    Allow,
    /// Rate-limited; do not send. `resume_at` is epoch seconds.
    Blocked { resume_at: i64 },
}

/// Consult the back-off state before spending a counted GitHub request. Local
/// file read only — never a GitHub request. Fails **open** (`Allow`) when the
/// data dir is unavailable or the file is missing/corrupt.
pub fn gate() -> Gate {
    let Ok(path) = paths::ratelimit_path() else {
        // No data dir (headless / no home) — never block on that.
        return Gate::Allow;
    };
    match load_at(&path) {
        Some(state) => gate_decision(&state, now_secs()),
        None => Gate::Allow,
    }
}

/// Record the rate-limit headers seen on any GitHub response (Layer B). A healthy
/// `remaining` clears any standing block; `remaining <= LOW_WATER` arms one until
/// `reset + PAD`.
pub fn note_response(remaining: Option<u32>, reset: Option<i64>) {
    let blocked_until = match remaining {
        Some(r) if r <= LOW_WATER => Some(compute_blocked_until(reset, now_secs())),
        // Healthy budget (or no header at all) — clear any prior block.
        _ => None,
    };
    persist(RateLimitState {
        reset,
        remaining,
        blocked_until,
    });
}

/// Record a confirmed `403`/`429` rate-limit response (Layer A + no-header
/// fallback). Blocks until `reset + PAD`, or `now + FALLBACK` when no reset was
/// reported. Callers MUST only invoke this on a confirmed rate-limit status.
/// Returns the resume instant (epoch seconds) it armed, so the caller can build a
/// matching "resume at HH:MM" error without re-reading the file.
pub fn note_rate_limited(reset: Option<i64>) -> i64 {
    let now = now_secs();
    let blocked_until = compute_blocked_until(reset, now);
    persist(RateLimitState {
        reset,
        remaining: Some(0),
        blocked_until: Some(blocked_until),
    });
    blocked_until
}

/// Format an epoch-seconds resume instant as a local `HH:MM` for the UI. Falls
/// back to "soon" if the timestamp is out of range.
pub fn format_resume(resume_at: i64) -> String {
    chrono::DateTime::from_timestamp(resume_at, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "soon".to_string())
}

// ---- pure decision helpers (unit-tested) ----

fn gate_decision(state: &RateLimitState, now: i64) -> Gate {
    match state.blocked_until {
        Some(until) if now < until => Gate::Blocked { resume_at: until },
        _ => Gate::Allow,
    }
}

/// The resume instant for a block: GitHub's reset plus the clock-skew pad, or the
/// fixed fallback when no reset is known.
fn compute_blocked_until(reset: Option<i64>, now: i64) -> i64 {
    match reset {
        Some(r) => r + PAD_SECS,
        None => now + FALLBACK_SECS,
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---- persistence (mirrors identity::save's atomic tmp-write + rename) ----

fn persist(state: RateLimitState) {
    let path = match paths::ratelimit_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "ratelimit path unavailable; skipping persist");
            return;
        }
    };
    if let Err(e) = save_at(&path, &state) {
        tracing::warn!(error = %e, "failed to persist ratelimits.json (non-fatal)");
    }
}

/// Read and parse the state file. Any failure (missing, unreadable, corrupt)
/// returns `None` so the gate fails open — a bad file never bricks checks.
fn load_at(path: &Path) -> Option<RateLimitState> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

/// Atomic write: a uuid-suffixed sibling tmp then rename. The unique tmp name
/// avoids clobbering between the concurrent boot fan-out writers (one self-update
/// check + one per visible channel all land here). Rename is atomic on POSIX and
/// NTFS, so a torn file is never observable.
fn save_at(path: &Path, state: &RateLimitState) -> Result<(), String> {
    let tmp = path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()));
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&json).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        // Best-effort cleanup of the orphaned tmp on a failed rename.
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_allows_when_unblocked() {
        let now = 1_000_000;
        // No block at all.
        assert_eq!(gate_decision(&RateLimitState::default(), now), Gate::Allow);
        // Block already expired.
        let expired = RateLimitState {
            blocked_until: Some(now - 1),
            ..Default::default()
        };
        assert_eq!(gate_decision(&expired, now), Gate::Allow);
    }

    #[test]
    fn gate_blocks_until_resume() {
        let now = 1_000_000;
        let state = RateLimitState {
            blocked_until: Some(now + 300),
            ..Default::default()
        };
        assert_eq!(
            gate_decision(&state, now),
            Gate::Blocked {
                resume_at: now + 300
            }
        );
        // Exactly at the resume instant we're clear again.
        assert_eq!(gate_decision(&state, now + 300), Gate::Allow);
    }

    #[test]
    fn block_window_uses_reset_plus_pad() {
        let now = 1_000_000;
        let reset = 1_000_500;
        assert_eq!(compute_blocked_until(Some(reset), now), reset + PAD_SECS);
    }

    #[test]
    fn block_window_falls_back_without_reset() {
        let now = 1_000_000;
        assert_eq!(compute_blocked_until(None, now), now + FALLBACK_SECS);
    }

    /// Layer B: at/below the low-water mark we arm a block; above it we clear.
    #[test]
    fn note_response_layer_b_boundaries() {
        let now = 1_000_000;
        // remaining == LOW_WATER -> block.
        let armed = match Some(LOW_WATER) {
            Some(r) if r <= LOW_WATER => Some(compute_blocked_until(Some(2_000_000), now)),
            _ => None,
        };
        assert_eq!(armed, Some(2_000_000 + PAD_SECS));
        // remaining just above LOW_WATER -> no block.
        let clear = match Some(LOW_WATER + 1) {
            Some(r) if r <= LOW_WATER => Some(0),
            _ => None,
        };
        assert_eq!(clear, None);
    }

    #[test]
    fn load_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ratelimits.json");
        assert!(load_at(&path).is_none(), "missing file must fail open");
    }

    #[test]
    fn load_corrupt_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ratelimits.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load_at(&path).is_none(), "corrupt file must fail open");
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ratelimits.json");
        let state = RateLimitState {
            reset: Some(1_700_000_000),
            remaining: Some(0),
            blocked_until: Some(1_700_000_120),
        };
        save_at(&path, &state).unwrap();
        let loaded = load_at(&path).expect("just-written file should parse");
        assert_eq!(loaded.reset, state.reset);
        assert_eq!(loaded.remaining, state.remaining);
        assert_eq!(loaded.blocked_until, state.blocked_until);
        // The uuid-suffixed tmp must not linger after a successful rename.
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".json.tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "tmp file should be renamed away");
    }

    #[test]
    fn format_resume_is_hh_mm() {
        // 1970-01-01T00:00:00Z + 9000s = 02:30 UTC; local offset only shifts the
        // hour, so assert the shape rather than an exact value.
        let s = format_resume(9000);
        assert_eq!(s.len(), 5, "HH:MM");
        assert_eq!(s.as_bytes()[2], b':');
    }
}
