//! Restarting the launcher after its own binary has been swapped.
//!
//! A successful self-update replaces the launcher on disk and the running
//! process then has to exit, because it is still executing the old code. Until
//! now it simply exited, leaving the user with no launcher and nothing telling
//! them to start it again. This module launches the replacement first.
//!
//! # The single-instance handshake
//!
//! The launcher claims a slot by writing a discovery file naming a loopback
//! port it listens on, and a second launcher that finds a *live* listener exits
//! as a duplicate (see [`crate::rendezvous`]). A child started while its parent
//! is still alive would therefore quit immediately, which is strictly worse
//! than the bug being fixed — the user would be left with nothing running at
//! all.
//!
//! So the child is started with [`AFTER_UPDATE_ARG`], and on seeing it `main`
//! waits for the slot instead of giving up on the first refusal. The parent
//! exits within milliseconds; its listener dies with it; the child's next probe
//! is refused and the existing stale-file reclaim takes over. No new
//! coordination mechanism, and a genuine second instance still loses the race
//! and exits, as it should.
//!
//! Waiting for the parent also fixes the Windows leftover problem: the swap
//! leaves a `.__relocated__.exe` that cannot be deleted while the old process
//! is running, and the child only reaches
//! [`super::cleanup_stale_update_artifacts`] after acquiring the slot — by
//! which time the parent is gone.

/// Argument passed to the replacement process, marking it as a post-update
/// relaunch rather than a user-initiated start.
pub const AFTER_UPDATE_ARG: &str = "--after-update";

/// Start the freshly swapped launcher, detached from this process.
///
/// Returns `Err` only if the spawn itself failed. The caller exits regardless:
/// a launcher that cannot restart itself is a worse outcome than one that
/// restarts, but it is still far better than continuing to run code whose
/// binary has already been replaced.
pub fn spawn_replacement() -> Result<(), String> {
    let mut cmd = build_command()?;
    cmd.arg(AFTER_UPDATE_ARG);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS (0x8) — no console is inherited or created, which
        // matters because release builds are `windows_subsystem = "windows"`.
        // CREATE_NEW_PROCESS_GROUP (0x200) — the child does not receive console
        // control events aimed at the process group this one belongs to.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn().map_err(|e| format!("spawn replacement launcher: {e}"))?;
    tracing::info!(pid = child.id(), "replacement launcher started");
    // The handle is dropped without waiting. On Unix the child is reparented to
    // init when this process exits moments later; on Windows the detached flags
    // above already cut it loose.
    Ok(())
}

/// The command that starts the new launcher, which is not always "the new
/// executable" — on macOS and Linux the thing that was swapped may be a bundle
/// or an AppImage rather than a bare binary.
fn build_command() -> Result<std::process::Command, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;

    #[cfg(target_os = "macos")]
    {
        // Go through Launch Services rather than exec'ing the Mach-O directly,
        // so the relaunched app gets a proper activation, Dock entry and
        // bundle identity. `-n` forces a new instance instead of reactivating
        // the one that is about to exit.
        if let Some(bundle) = super::github::bundle_root(&exe) {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-n").arg(&bundle).arg("--args");
            return Ok(cmd);
        }
    }

    #[cfg(target_os = "linux")]
    {
        // An AppImage runs from a read-only mount that disappears with the
        // process; the thing to restart is the outer .AppImage file that the
        // swap replaced, whose path the runtime exports.
        if let Ok(appimage) = std::env::var("APPIMAGE") {
            return Ok(std::process::Command::new(appimage));
        }
    }

    // Bare binary: `current_exe` now resolves to the replacement, because the
    // swap put the new file at the old path.
    Ok(std::process::Command::new(exe))
}
