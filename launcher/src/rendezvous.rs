//! Socket rendezvous — ephemeral-port single-instance + cross-restart liveness.
//!
//! One mechanism covers three needs:
//!   1. **Single launcher instance** — a second launcher detects the first and
//!      exits cleanly.
//!   2. **Game liveness across a launcher restart** — the launcher can tell a
//!      game it (or a previous launcher) spawned is still running, and correctly
//!      tell a *crashed* game from a live one.
//!   3. (game side, in C#) **one game channel at a time**.
//!
//! How it avoids colliding with strangers' software: each holder binds
//! `127.0.0.1:0`, so the OS hands back a guaranteed-free **ephemeral** port —
//! no fixed port to clash with. The chosen port is recorded in a small
//! *discovery file* under the per-user data dir; a *handshake banner* proves the
//! listener is really us (not a stranger the OS recycled the port to); and the
//! slot is claimed with an atomic `create_new` (mirroring the handoff file's
//! `create_new(true)` in `game_launch/mod.rs`), ordered **bind-before-claim** so
//! a second starter that sees the file always gets an answer.
//!
//! Protocol mirror — keep in sync with `client/src/core/SingleInstance.cs`:
//!   - proto version = [`RENDEZVOUS_PROTO`]
//!   - banner line per accepted connection: `BRISKA-BLAST\t{proto}\t{ROLE}\n`
//!   - discovery file content: `{"port":<u16>,"proto":1}`
//!
//! **Fail-safe is the prime directive:** any inconclusive error here logs and
//! lets the launcher run normally — single-instance is a convenience, never a
//! gate that can wedge the app shut.

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

/// Wire protocol version. MUST match `SingleInstance.Proto` in the C# client.
pub const RENDEZVOUS_PROTO: u32 = 1;

const ROLE_LAUNCHER: &str = "LAUNCHER";
const ROLE_GAME: &str = "GAME";
const LAUNCHER_FILE: &str = "launcher_instance.json";
const GAME_FILE: &str = "game_instance.json";

/// Connect + read timeout for a probe. Short — a live local listener answers in
/// well under this; the cap just bounds a refused/filtered connect so a probe
/// can never hang the boot or a poll tick.
const PROBE_TIMEOUT: Duration = Duration::from_millis(200);
/// Bounded `create_new` retries: each `AlreadyExists` either proves a live
/// instance (→ duplicate) or reclaims a stale file (→ retry).
const ACQUIRE_ATTEMPTS: usize = 5;
/// When a claimed file exists but yields no parseable port yet, the owner may be
/// mid-init (it `create_new`'d then is about to write the port). Re-read a few
/// times before declaring it stale so the create→write window can't trigger a
/// false reclaim that would double-instance us.
const PORT_READ_ATTEMPTS: usize = 10;
const PORT_READ_BACKOFF: Duration = Duration::from_millis(10);

/// Discovery-file payload. Mirrors the C# `{"port":…,"proto":…}` shape.
#[derive(Debug, Serialize, Deserialize)]
struct Discovery {
    port: u16,
    proto: u32,
}

/// The banner a holder of `role` writes on each accepted connection.
fn banner(role: &str) -> String {
    format!("BRISKA-BLAST\t{RENDEZVOUS_PROTO}\t{role}\n")
}

/// Outcome of an acquire attempt.
pub enum AcquireOutcome {
    /// This process should run. Carries a guard that, on drop, removes the
    /// discovery file iff we actually own it (clean-exit cleanup). A fail-safe
    /// "couldn't claim, run anyway" also lands here, with a non-owning guard.
    Live(InstanceGuard),
    /// Another live instance already holds the slot — the caller should exit.
    Duplicate,
}

/// Removes the discovery file on a clean exit — but only if this process won the
/// claim. A duplicate (which never owns the file) and a fail-safe non-claim both
/// hold a `None` guard and delete nothing. A crash/kill skips Drop entirely, so
/// the file is left behind and the next probe self-corrects (connect refused).
pub struct InstanceGuard {
    /// `Some` only when we created/reclaimed the file ourselves.
    owned_file: Option<PathBuf>,
}

impl InstanceGuard {
    fn owning(path: PathBuf) -> Self {
        Self {
            owned_file: Some(path),
        }
    }
    /// A guard that owns nothing — used for every fail-safe "run normally"
    /// path so the app proceeds without single-instance protection.
    fn no_claim() -> Self {
        Self { owned_file: None }
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Some(path) = self.owned_file.take() {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "failed to remove instance file on exit")
                }
            }
        }
    }
}

/// Acquire (or detect a duplicate of) the launcher single-instance slot. Sync —
/// runs in `main()` before Iced starts its tokio runtime, so it uses blocking
/// `std::net`. On the `Live` path the listener is owned by a detached accept
/// thread that serves the banner for the whole process.
pub fn acquire_launcher() -> AcquireOutcome {
    let dir = match crate::paths::data_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "data dir unavailable; launcher single-instance disabled (fail-safe)");
            return AcquireOutcome::Live(InstanceGuard::no_claim());
        }
    };
    acquire_in(&dir, ROLE_LAUNCHER, LAUNCHER_FILE)
}

/// Liveness probe of the game's discovery file (no bind, no claim — read +
/// connect only). `true` only when a process answering the `GAME` banner is
/// live. Runs on Iced's tokio runtime via `Task::perform`.
pub async fn game_is_running() -> bool {
    let dir = match crate::paths::data_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "data dir unavailable; treating game as not running (fail-safe)");
            return false;
        }
    };
    probe(&dir.join(GAME_FILE), ROLE_GAME).await
}

/// Core acquire, parameterised on the directory so tests can target a tempdir.
fn acquire_in(dir: &Path, role: &'static str, file_name: &str) -> AcquireOutcome {
    let path = dir.join(file_name);

    // Bind FIRST: `TcpListener::bind` starts listening immediately, so by the
    // time the discovery file exists below we already answer connects. That
    // closes the init window — a second starter that sees our file can never
    // mistake a still-initialising instance for a stale one.
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %e, "could not bind loopback listener; single-instance disabled (fail-safe)");
            return AcquireOutcome::Live(InstanceGuard::no_claim());
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            tracing::warn!(error = %e, "could not read listener addr; single-instance disabled (fail-safe)");
            return AcquireOutcome::Live(InstanceGuard::no_claim());
        }
    };

    for _ in 0..ACQUIRE_ATTEMPTS {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                // We claimed the slot. Write the port immediately, then start
                // serving the banner.
                let payload = serde_json::to_vec(&Discovery {
                    port,
                    proto: RENDEZVOUS_PROTO,
                })
                .unwrap_or_default();
                if let Err(e) = f.write_all(&payload).and_then(|()| f.flush()) {
                    tracing::warn!(error = %e, "failed to write discovery file; single-instance disabled (fail-safe)");
                    let _ = std::fs::remove_file(&path);
                    return AcquireOutcome::Live(InstanceGuard::no_claim());
                }
                tracing::info!(port, role, "acquired single-instance slot");
                spawn_accept_loop(listener, role);
                return AcquireOutcome::Live(InstanceGuard::owning(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                match read_port_with_retry(&path) {
                    Some(p) if connect_banner_matches_sync(p, role) => {
                        tracing::info!(role, "live instance detected — this process is a duplicate");
                        return AcquireOutcome::Duplicate;
                    }
                    // File exists but nothing live answers (crashed instance,
                    // recycled port, stranger, or never wrote a port) → stale.
                    _ => {
                        tracing::info!(role, "stale instance file — reclaiming");
                        remove_stale(&path);
                        continue;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "unexpected error claiming instance file; single-instance disabled (fail-safe)");
                return AcquireOutcome::Live(InstanceGuard::no_claim());
            }
        }
    }

    tracing::warn!(role, "exhausted instance-claim attempts; single-instance disabled (fail-safe)");
    AcquireOutcome::Live(InstanceGuard::no_claim())
}

/// Detached thread that owns `listener` for the process lifetime and writes the
/// role banner on every accepted connection. If it can't spawn (effectively
/// only under OOM) we log and proceed: the slot stays claimed but unanswered, so
/// a future probe treats it as stale — acceptable for such a rare case.
fn spawn_accept_loop(listener: TcpListener, role: &'static str) {
    let banner = banner(role).into_bytes();
    let spawned = std::thread::Builder::new()
        .name(format!("briska-rendezvous-{}", role.to_ascii_lowercase()))
        .spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(mut stream) => {
                        let _ = stream.write_all(&banner);
                        let _ = stream.flush();
                        // `stream` drops here, closing the connection.
                    }
                    // A transient per-connection accept error must not kill the
                    // listener — keep serving.
                    Err(_) => continue,
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not spawn rendezvous accept thread (fail-safe)");
    }
}

/// Read the discovery file's port, retrying briefly while it is unparseable. A
/// freshly-`create_new`'d file is momentarily empty/partial before the owner
/// writes the port; without this retry a probe could read that transient state
/// and wrongly reclaim a live slot. A genuinely stale/corrupt file simply
/// exhausts the retries and returns `None`.
fn read_port_with_retry(path: &Path) -> Option<u16> {
    for attempt in 0..PORT_READ_ATTEMPTS {
        if let Some(p) = read_port(path) {
            return Some(p);
        }
        if attempt + 1 < PORT_READ_ATTEMPTS {
            std::thread::sleep(PORT_READ_BACKOFF);
        }
    }
    None
}

/// Parse the discovery file's port. `None` on missing / unreadable / unparseable.
fn read_port(path: &Path) -> Option<u16> {
    let s = std::fs::read_to_string(path).ok()?;
    let disc: Discovery = serde_json::from_str(&s).ok()?;
    Some(disc.port)
}

fn remove_stale(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(error = %e, "failed to remove stale instance file"),
    }
}

/// Blocking connect-and-check: `true` iff a listener on `port` answers with the
/// banner for `role`. Used by the sync acquire path. Any error → `false`.
fn connect_banner_matches_sync(port: u16, role: &str) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = match TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let expected = banner(role);
    let mut got = Vec::with_capacity(expected.len());
    let mut buf = [0u8; 64];
    while got.len() < expected.len() {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    got.starts_with(expected.as_bytes())
}

/// Async liveness probe of a discovery file for `role`. `true` iff a live
/// listener answers with that role's banner within [`PROBE_TIMEOUT`]. Never
/// hangs (the timeout bounds connect + read); any failure → `false`.
async fn probe(path: &Path, role: &str) -> bool {
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpStream as TokioTcpStream;

    let Some(port) = read_port(path) else {
        return false;
    };
    let expected = banner(role);

    let read_banner = async {
        let mut stream = TokioTcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .ok()?;
        let mut got = Vec::with_capacity(expected.len());
        let mut buf = [0u8; 64];
        while got.len() < expected.len() {
            match stream.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        Some(got)
    };

    match tokio::time::timeout(PROBE_TIMEOUT, read_banner).await {
        Ok(Some(got)) => got.starts_with(expected.as_bytes()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind a real banner-serving listener for `role` and write a matching
    /// discovery file at `path`. Returns the guard-less listener thread's port;
    /// the listener lives until the process exits (detached), which is fine for
    /// a test binary.
    fn serve_banner(path: &Path, role: &'static str) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        spawn_accept_loop(listener, role);
        std::fs::write(
            path,
            serde_json::to_vec(&Discovery {
                port,
                proto: RENDEZVOUS_PROTO,
            })
            .unwrap(),
        )
        .unwrap();
        port
    }

    /// A free port that nothing is listening on: bind then drop, so the OS has
    /// just handed it back as free and a connect will be refused promptly.
    fn dead_port() -> u16 {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    }

    #[tokio::test]
    async fn probe_false_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!probe(&tmp.path().join("game_instance.json"), ROLE_GAME).await);
    }

    #[tokio::test]
    async fn probe_true_against_matching_banner() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game_instance.json");
        serve_banner(&path, ROLE_GAME);
        assert!(probe(&path, ROLE_GAME).await);
    }

    #[tokio::test]
    async fn probe_false_against_wrong_banner() {
        // Stranger guard: a listener that answers with a *different* role's
        // banner must not be mistaken for the one we're probing for.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game_instance.json");
        serve_banner(&path, ROLE_LAUNCHER);
        assert!(!probe(&path, ROLE_GAME).await);
    }

    #[tokio::test]
    async fn probe_false_on_refused_connect_without_hanging() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("game_instance.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&Discovery {
                port: dead_port(),
                proto: RENDEZVOUS_PROTO,
            })
            .unwrap(),
        )
        .unwrap();
        // Generous outer bound asserts the inner PROBE_TIMEOUT actually fires
        // rather than the read blocking forever.
        let res = tokio::time::timeout(Duration::from_secs(2), probe(&path, ROLE_GAME))
            .await
            .expect("probe must return within the timeout");
        assert!(!res);
    }

    #[test]
    fn acquire_wins_on_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LAUNCHER_FILE);
        match acquire_in(tmp.path(), ROLE_LAUNCHER, LAUNCHER_FILE) {
            AcquireOutcome::Live(_g) => assert!(path.exists(), "winner writes the discovery file"),
            AcquireOutcome::Duplicate => panic!("empty dir must produce a Live acquire"),
        }
    }

    #[test]
    fn second_acquire_is_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        // Hold the first guard (and its accept thread) alive across the second
        // attempt so the second can connect and read the live banner.
        let _first = match acquire_in(tmp.path(), ROLE_LAUNCHER, LAUNCHER_FILE) {
            AcquireOutcome::Live(g) => g,
            AcquireOutcome::Duplicate => panic!("first acquire must win"),
        };
        assert!(matches!(
            acquire_in(tmp.path(), ROLE_LAUNCHER, LAUNCHER_FILE),
            AcquireOutcome::Duplicate
        ));
    }

    #[test]
    fn acquire_reclaims_stale_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LAUNCHER_FILE);
        // A leftover file pointing at a dead port == crashed prior instance.
        std::fs::write(
            &path,
            serde_json::to_vec(&Discovery {
                port: dead_port(),
                proto: RENDEZVOUS_PROTO,
            })
            .unwrap(),
        )
        .unwrap();
        match acquire_in(tmp.path(), ROLE_LAUNCHER, LAUNCHER_FILE) {
            AcquireOutcome::Live(_g) => {
                // Reclaimed and rewritten with our own (live) port.
                assert!(read_port(&path).is_some());
            }
            AcquireOutcome::Duplicate => panic!("a dead-port file must be reclaimed, not treated as live"),
        }
    }

    #[test]
    fn concurrent_acquire_has_exactly_one_winner() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let dir = dir.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                match acquire_in(&dir, ROLE_LAUNCHER, LAUNCHER_FILE) {
                    // Return the guard so the winner's listener stays alive
                    // until both threads have finished racing.
                    AcquireOutcome::Live(g) => (true, Some(g)),
                    AcquireOutcome::Duplicate => (false, None),
                }
            }));
        }
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let wins = results.iter().filter(|(live, _)| *live).count();
        assert_eq!(wins, 1, "exactly one acquirer must win the race");
    }
}
