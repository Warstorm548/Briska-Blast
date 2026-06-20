//! Running-game memory file.
//!
//! The launcher spawns the game as a tracked child and holds the `Child` handle
//! in memory for the normal case (the `spawn_and_wait` task in `game_launch`
//! drives the `GameExited` message when it returns). That handle is lost if the
//! launcher itself is closed and reopened **while the game is still running** —
//! a fresh launcher would then forget a game is live and wrongly re-enable the
//! game-gated buttons (Play, Change Name, Uninstall, …).
//!
//! This module is the cross-restart fallback: a tiny `running_game.json` next to
//! `identity.json` (see `paths::running_game_path`) recording the spawned PID.
//! Because a *handle* can't be persisted, we store the **PID** plus enough
//! metadata to defeat PID reuse — the OS recycles PIDs, so a naive "is this PID
//! alive?" check could match a later, unrelated process and lock the UI forever.
//! On recovery we therefore require the live process's **start time** to sit
//! within tolerance of our recorded spawn instant, and (best-effort) its **exe
//! name** to match.
//!
//! Like `ratelimit.rs`, the file **fails safe**: a missing or corrupt file reads
//! as "no game running" so a bad file can never permanently lock the launcher.
//! Persistence mirrors `identity::save` / `ratelimit::save_at` — write a
//! uuid-suffixed sibling tmp, then atomically rename.

use crate::channel::Channel;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Allowed gap (seconds) between the OS-reported process start time and the
/// launcher's recorded spawn instant. The two are taken moments apart, so any
/// real gap is just scheduling + whole-second clock granularity; a recycled PID
/// belonging to a later process lands far outside this window.
const START_TOLERANCE_SECS: i64 = 30;

/// Persisted record of the game the launcher last spawned and believes may still
/// be open. PID alone is ambiguous across restarts (reuse), so it is paired with
/// the spawn instant and exe for the liveness check in [`is_alive`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunningGame {
    /// OS process id of the spawned game.
    pub pid: u32,
    /// Epoch seconds when the launcher spawned it (launcher wall clock). Paired
    /// with the live process's start time at recovery to defeat PID reuse.
    pub spawned_at: i64,
    /// Full path to the launched game executable — a secondary identity check.
    pub exe: PathBuf,
    /// Channel whose game is running (lets the UI label it, e.g. "Running (EA)").
    pub channel: Channel,
}

impl RunningGame {
    /// Build a record stamped with the current wall-clock spawn instant.
    pub fn now(pid: u32, exe: PathBuf, channel: Channel) -> Self {
        Self {
            pid,
            spawned_at: chrono::Utc::now().timestamp(),
            exe,
            channel,
        }
    }
}

/// Persist the running-game record. Best-effort: a failure is logged and
/// swallowed — losing the file only costs the cross-restart recovery, never the
/// running session (whose authority is the in-memory child handle).
pub fn write(rec: &RunningGame) {
    let path = match paths::running_game_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "running_game path unavailable; skipping write");
            return;
        }
    };
    if let Err(e) = save_at(&path, rec) {
        tracing::warn!(error = %e, "failed to persist running_game.json (non-fatal)");
    }
}

/// Remove the running-game record — on normal game exit, or when recovery finds
/// the file stale. A missing file is success.
pub fn clear() {
    let path = match paths::running_game_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(error = %e, "failed to remove running_game.json (non-fatal)"),
    }
}

/// Read the persisted record, if any. `None` on missing/corrupt — fails safe to
/// "no game running" so a bad file can never permanently lock the UI.
pub fn load() -> Option<RunningGame> {
    let path = paths::running_game_path().ok()?;
    load_at(&path)
}

/// Boot-time recovery: load the record and confirm the process is still our
/// game. Returns `Some` only when it is genuinely still alive; otherwise removes
/// the now-stale file and returns `None`, so a crashed game or a reused PID can
/// never leave the buttons locked.
pub fn recover() -> Option<RunningGame> {
    let rec = load()?;
    if is_alive(&rec) {
        Some(rec)
    } else {
        clear();
        None
    }
}

/// Is the recorded process still alive **and** actually our game? Requires: the
/// PID to be present; its start time within [`START_TOLERANCE_SECS`] of our
/// recorded spawn instant (the PID-reuse guard); and — when both sides are
/// resolvable — a matching exe file name. An unresolvable exe (perms / platform)
/// does not by itself reject, since the start-time guard already disambiguates.
pub fn is_alive(rec: &RunningGame) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};

    let pid = Pid::from_u32(rec.pid);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    let Some(proc_) = sys.process(pid) else {
        return false;
    };

    let start_ok = (proc_.start_time() as i64 - rec.spawned_at).abs() <= START_TOLERANCE_SECS;
    let exe_ok = match (proc_.exe().and_then(|p| p.file_name()), rec.exe.file_name()) {
        (Some(live), Some(want)) => live == want,
        // Can't resolve one side — rely on the start-time guard alone.
        _ => true,
    };
    start_ok && exe_ok
}

// ---- persistence (mirrors ratelimit::save_at / identity::save) ----

/// Atomic write: a uuid-suffixed sibling tmp then rename, so a torn file is
/// never observable. Rename is atomic on POSIX and NTFS.
fn save_at(path: &Path, rec: &RunningGame) -> Result<(), String> {
    let tmp = path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()));
    let json = serde_json::to_vec_pretty(rec).map_err(|e| e.to_string())?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&json).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Read and parse the record. Any failure (missing, unreadable, corrupt) returns
/// `None` so recovery fails safe.
fn load_at(path: &Path) -> Option<RunningGame> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysinfo::{Pid, ProcessesToUpdate, System};

    fn sample(pid: u32, spawned_at: i64) -> RunningGame {
        RunningGame {
            pid,
            spawned_at,
            exe: std::env::current_exe().unwrap(),
            channel: Channel::Dev,
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("running_game.json");
        let rec = sample(4242, 1_700_000_000);
        save_at(&path, &rec).unwrap();
        assert_eq!(load_at(&path).expect("just-written file parses"), rec);
        // The uuid-suffixed tmp must not linger after a successful rename.
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".json.tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "tmp file should be renamed away");
    }

    #[test]
    fn load_missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("running_game.json");
        assert!(load_at(&path).is_none(), "missing file must fail safe");
    }

    #[test]
    fn load_corrupt_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("running_game.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load_at(&path).is_none(), "corrupt file must fail safe");
    }

    /// The current process is, by definition, alive — and when stamped with its
    /// real start time it passes both the liveness and the start-time guard.
    #[test]
    fn is_alive_true_for_current_process() {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        // Stamp spawned_at with our actual start time so the match is exact
        // regardless of how long the test suite has been running.
        let start = sys.process(pid).unwrap().start_time() as i64;
        assert!(is_alive(&sample(std::process::id(), start)));
    }

    /// PID-reuse guard: the PID is live (it's us) but the recorded spawn instant
    /// is decades off, so it must NOT be treated as our game.
    #[test]
    fn is_alive_rejects_mismatched_start_time() {
        assert!(!is_alive(&sample(std::process::id(), 1_000_000)));
    }

    /// A PID that almost certainly doesn't exist reads as dead.
    #[test]
    fn is_alive_false_for_absent_pid() {
        assert!(!is_alive(&sample(4_000_000_000, chrono::Utc::now().timestamp())));
    }
}
