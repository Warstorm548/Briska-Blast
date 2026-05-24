//! Spawn the installed game executable with a one-shot identity handoff.
//!
//! Protocol mirror of Stage 1's `client/src/core/LaunchArgs.cs`:
//!   1. Launcher writes `${TMPDIR}/briskablast-handoff-<uuid>.json`
//!      containing `{"username": "..."}` (perms `0600` on unix).
//!   2. Launcher spawns the game executable with
//!      `--launcher-handoff <path>` as its single arg.
//!   3. Game reads + **deletes** the file on startup; if the game crashes
//!      before that, the file is left behind. Acceptable for v1 —
//!      cleaning it from the launcher side races against the game's read.
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

    let handoff_path = write_handoff(&username).await?;
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

    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait on game process: {e}"))?;
    tracing::info!(?channel, ?status, "game process exited");

    // If the game never read the handoff file, clean it up now so we
    // don't leave secrets-shaped temp files around. `tokio::fs::remove_file`
    // is fine if the file is already gone — we map NotFound to Ok.
    if let Err(e) = tokio::fs::remove_file(&handoff_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                error = %e,
                path = %handoff_path.display(),
                "failed to remove handoff temp file"
            );
        }
    }

    Ok(status.code())
}

async fn write_handoff(username: &str) -> Result<PathBuf, String> {
    let name = format!("briskablast-handoff-{}.json", uuid::Uuid::new_v4());
    let path = std::env::temp_dir().join(name);

    let payload = serde_json::to_vec(&Handoff { username })
        .map_err(|e| format!("serialize handoff: {e}"))?;

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

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(&path, perms) {
            // Non-fatal — the handoff is short-lived and only contains
            // the username (no secret yet, per Stage 1). Warn and move on.
            tracing::warn!(error = %e, path = %path.display(), "chmod 0600 handoff failed");
        }
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handoff_roundtrips_username() {
        let path = write_handoff("BlastQueen99").await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["username"], "BlastQueen99");
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn handoff_paths_are_unique() {
        let a = write_handoff("x").await.unwrap();
        let b = write_handoff("x").await.unwrap();
        assert_ne!(a, b);
        let _ = tokio::fs::remove_file(&a).await;
        let _ = tokio::fs::remove_file(&b).await;
    }

}
