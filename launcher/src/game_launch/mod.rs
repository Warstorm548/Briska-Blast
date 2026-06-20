//! Spawn the installed game executable with a one-shot identity handoff.
//!
//! Protocol mirror of `client/src/core/LaunchArgs.cs`:
//!   1. Launcher writes `${TMPDIR}/briskablast-handoff-<uuid>.json`
//!      containing the channel's identity + the launcher version (perms
//!      `0600` on unix — the file now carries the secret token).
//!   2. Launcher spawns the game executable with
//!      `--launcher-handoff <path>` as its single arg.
//!   3. Game reads + **deletes** the file on startup; if the game crashes
//!      before that, the file is left behind. Acceptable for v1 —
//!      cleaning it from the launcher side races against the game's read.
//!
//! Why the extra fields: every server call the client makes (`/host`,
//! `/join`, the WS `identify` frame) needs `player_id` + `secret_token`,
//! and the version gate (`server/src/middleware/version.rs`) checks
//! `X-Launcher-Version` too — which the game has no way to know on its
//! own. The `channel` lets the client assert it wasn't handed credentials
//! for a different channel than the one it was built for.
//!
//! `spawn_and_wait` is the single public entry point: writes the file,
//! spawns the binary, awaits exit, returns the exit code. Errors at any
//! step surface as a single `Err(String)`.

use crate::channel::Channel;
use crate::updater::branches::installed_manifest;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const HANDOFF_FLAG: &str = "--launcher-handoff";

#[derive(Debug, Clone, Serialize)]
struct Handoff<'a> {
    username: &'a str,
    player_id: &'a str,
    secret_token: &'a str,
    launcher_version: &'a str,
    channel: Channel,
}

/// Spawn the channel's installed game, wait for it to exit. The handoff
/// file is written before the spawn and removed by the game on read.
///
/// Returns the exit code (or a synthesised one on signal exits) so the app
/// can log it; any errors before the child actually starts (manifest
/// missing, exe missing, etc) bubble up as `Err`.
pub async fn spawn_and_wait(
    channel: Channel,
    install_dir: PathBuf,
    username: String,
    player_id: String,
    secret_token: String,
    launcher_version: String,
) -> Result<Option<i32>, String> {
    let manifest = installed_manifest(&install_dir)
        .await
        .map_err(|e| format!("read installed.json: {e}"))?
        .ok_or_else(|| {
            format!(
                "no installed.json under {} — channel may not be installed",
                install_dir.display()
            )
        })?;

    let exe_path = install_dir.join(&manifest.executable);
    if !exe_path.exists() {
        return Err(format!(
            "executable {} listed in manifest does not exist on disk",
            exe_path.display()
        ));
    }

    let handoff_path = write_handoff(&Handoff {
        username: &username,
        player_id: &player_id,
        secret_token: &secret_token,
        launcher_version: &launcher_version,
        channel,
    })
    .await?;
    // RAII guard — removes the handoff file on scope exit (including the
    // early-return `?` from spawn / wait failures below) so a crashing
    // launcher can't leave secret-shaped temp files behind. If the game
    // already consumed and deleted it, the remove just sees NotFound and
    // is silently ignored by the Drop impl.
    let _handoff_cleanup = HandoffCleanup::new(handoff_path.clone());

    tracing::info!(
        ?channel,
        exe = %exe_path.display(),
        handoff = %handoff_path.display(),
        "spawning game"
    );

    let mut cmd = Command::new(&exe_path);
    cmd.arg(HANDOFF_FLAG)
        .arg(&handoff_path)
        // Inherit stdio so any GD.Print output is visible when the
        // launcher itself is being run from a terminal.
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // Run the game from its install dir so relative resource paths resolve.
    cmd.current_dir(&install_dir);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe_path.display()))?;

    // Record the running game so a launcher restart mid-game can recover it and
    // keep the game-gated buttons locked. The PID only exists post-spawn. This is
    // best-effort: losing it only costs the cross-restart recovery, never the
    // running session (the in-memory child handle below is authoritative). The
    // file is removed by the `GameExited` handler once `wait()` returns; it is
    // deliberately left behind if the *launcher* dies first — that is the case
    // boot recovery exists for.
    match child.id() {
        Some(pid) => crate::running_game::write(&crate::running_game::RunningGame::now(
            pid,
            exe_path.clone(),
            channel,
        )),
        None => tracing::warn!("spawned game has no pid — skipping running_game memory file"),
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait on game process: {e}"))?;
    tracing::info!(?channel, ?status, "game process exited");

    // Cleanup happens via the HandoffCleanup Drop impl when this function
    // returns (success or error). No explicit remove_file call needed.
    Ok(status.code())
}

/// RAII guard that removes a handoff temp file when dropped. Uses the
/// blocking `std::fs::remove_file` because Drop can't be async; the file
/// is small and the operation is microseconds. Silently ignores NotFound
/// (game already deleted it) and warns on any other error.
struct HandoffCleanup {
    path: Option<PathBuf>,
}

impl HandoffCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

impl Drop for HandoffCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "failed to remove handoff temp file on cleanup"
                    );
                }
            }
        }
    }
}

async fn write_handoff(handoff: &Handoff<'_>) -> Result<PathBuf, String> {
    let name = format!("briskablast-handoff-{}.json", uuid::Uuid::new_v4());
    let path = std::env::temp_dir().join(name);

    let payload = serde_json::to_vec(handoff)
        .map_err(|e| format!("serialize handoff: {e}"))?;

    // On unix, atomically create the file with 0o600 from the start so a
    // racing local process can't observe it world-readable in the window
    // between create() and a chmod. create_new(true) also makes us bail
    // (rather than overwrite) if the path was somehow pre-created — extra
    // belt for the random-uuid suspenders. On non-unix targets we fall
    // back to the standard create — Windows file perms aren't expressible
    // through OpenOptionsExt. The payload now carries the secret_token, so
    // the Windows plaintext-in-temp exposure is a known v1 gap (the file is
    // uuid-named and short-lived; see the WS-ticket roadmap entry for the
    // hardening that removes the raw token from the wire entirely).
    #[cfg(unix)]
    let mut f = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .await
        .map_err(|e| format!("create handoff: {e}"))?;
    #[cfg(not(unix))]
    let mut f = tokio::fs::File::create(&path)
        .await
        .map_err(|e| format!("create handoff: {e}"))?;

    f.write_all(&payload)
        .await
        .map_err(|e| format!("write handoff: {e}"))?;
    f.flush()
        .await
        .map_err(|e| format!("flush handoff: {e}"))?;
    drop(f);

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(username: &'static str) -> Handoff<'static> {
        Handoff {
            username,
            player_id: "000000042",
            secret_token: "deadbeefcafef00d",
            launcher_version: "0.10.0",
            channel: Channel::Dev,
        }
    }

    #[tokio::test]
    async fn handoff_roundtrips_all_fields() {
        let path = write_handoff(&sample("BlastQueen99")).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["username"], "BlastQueen99");
        assert_eq!(v["player_id"], "000000042");
        assert_eq!(v["secret_token"], "deadbeefcafef00d");
        assert_eq!(v["launcher_version"], "0.10.0");
        assert_eq!(v["channel"], "dev");
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn handoff_paths_are_unique() {
        let a = write_handoff(&sample("x")).await.unwrap();
        let b = write_handoff(&sample("x")).await.unwrap();
        assert_ne!(a, b);
        let _ = tokio::fs::remove_file(&a).await;
        let _ = tokio::fs::remove_file(&b).await;
    }

}
