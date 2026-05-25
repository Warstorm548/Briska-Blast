//! Windows Firewall inbound-rule detection for the per-channel game exe.
//!
//! The game uses WebRTC / NAT hole-punching (see
//! docs/architecture/game-architecture-summary.md). On Windows the first time
//! the game binds a listening socket, Windows Firewall either prompts (needs
//! admin) or silently blocks inbound, which can break hosting in non-obvious
//! ways. The game is NOT installed by the NSIS installer — the launcher
//! downloads it per-channel at runtime, so its path is dynamic and unknown at
//! install time. A firewall rule therefore has to be the launcher's
//! responsibility, and creating one requires elevation.
//!
//! This module has two halves. `inbound_rule_status` is the **non-elevated
//! detection** — it reports whether an inbound rule for a given exe already
//! exists, with no admin rights required to look. `add_inbound_rule_elevated`
//! is the **elevated write** (option A): a single UAC elevation that runs
//! `netsh … add rule` to create the inbound allow rule. The write is invoked
//! from the first-Play prompt (see `app.rs`), keeping it user-initiated to
//! mirror the project's user-initiated-update philosophy.
//!
//! On Linux, outbound-initiated hole-punching traverses the default host
//! firewall (ufw doesn't block outbound; inbound for hole-punched flows is
//! solicited), so there is nothing to detect or manipulate. The detection
//! function returns `NotApplicable` there and no Linux firewall code exists
//! in this module by design.

use crate::channel::Channel;

/// Result of a non-elevated inbound-rule lookup for one game exe.
// `RulePresent`/`NotDetected` are constructed only on the Windows code path;
// on other targets the detection fn yields `NotApplicable`, so without this
// they read as dead on Linux even though the UI matches on them.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallStatus {
    /// An inbound rule referencing this exe was found.
    RulePresent,
    /// The firewall has no inbound rule referencing this exe.
    NotDetected,
    /// The check itself couldn't complete (netsh missing / errored). The
    /// string is a diagnostic for logs; the UI shows a short label.
    Unknown(#[allow(dead_code)] String),
    /// Not a Windows host — inbound rules are not the launcher's concern here.
    // Mirror of the enum-level attribute: `NotApplicable` is constructed only on
    // the non-Windows stub, so on Windows it would read as dead even though the
    // UI matches on it.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    NotApplicable,
}

/// Check whether a Windows Firewall inbound rule already references `game_exe`.
/// Read-only and **non-elevated** — `netsh advfirewall firewall show` needs no
/// admin rights. We list all inbound rules and look for the exe path within
/// the output; matching the path itself (locale-independent) rather than the
/// localized "Program:" field label keeps this working on non-English Windows.
#[cfg(target_os = "windows")]
pub async fn inbound_rule_status(game_exe: std::path::PathBuf) -> FirewallStatus {
    use tokio::process::Command;

    let needle = game_exe.to_string_lossy().to_lowercase();
    let output = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "show",
            "rule",
            "name=all",
            "dir=in",
            "verbose",
        ])
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            // A positive match is conclusive regardless of exit status.
            if stdout.contains(&needle) {
                FirewallStatus::RulePresent
            } else if out.status.success() {
                // netsh ran fine and the exe isn't referenced by any rule.
                FirewallStatus::NotDetected
            } else {
                // Non-zero exit with no match means we genuinely couldn't tell
                // (access denied, bad args, an empty rule table reported as an
                // error, …). Surface it as Unknown with netsh's own output
                // rather than misreporting a confident "no rule".
                let stderr = String::from_utf8_lossy(&out.stderr);
                FirewallStatus::Unknown(format!(
                    "netsh exited {}: {}",
                    out.status,
                    stderr.trim()
                ))
            }
        }
        Err(e) => FirewallStatus::Unknown(format!("netsh invocation failed: {e}")),
    }
}

/// Non-Windows stub — see module docs (Linux uses outbound hole-punching).
#[cfg(not(target_os = "windows"))]
pub async fn inbound_rule_status(_game_exe: std::path::PathBuf) -> FirewallStatus {
    FirewallStatus::NotApplicable
}

/// Locale-stable inbound-rule name for a channel's game. Built only from the
/// fixed `Channel` enum (never user text), so interpolating it into the
/// elevated `netsh` command carries no injection surface. Kept un-gated so the
/// UI and tests can show/assert the name on every platform.
pub fn rule_name(channel: Channel) -> String {
    format!("BriskaBlast {} Game", channel.dir_name())
}

/// Create the inbound allow rule for `game_exe`, elevated. This triggers a
/// single UAC prompt (option A) and runs, as admin, the equivalent of:
///
///   netsh advfirewall firewall add rule name="BriskaBlast <channel> Game" \
///       dir=in action=allow program="<game_exe>" enable=yes
///
/// `runas` performs the `ShellExecuteExW`/"runas" elevation, waits on the
/// process, and returns its exit code. Each `name=` / `program=` token is
/// passed as a discrete arg — `runas` quotes/escapes args containing spaces or
/// quotes, and netsh's CRT parser splits them back, so a path with spaces is
/// handled and there is no command-injection surface (paths cannot contain the
/// `"`/`<`/`>`/`|` that would be needed to break out, and the rule name is
/// enum-derived).
///
/// **Blocking** — `ShellExecuteExW` + `WaitForSingleObject` run synchronously,
/// so callers must invoke this inside `tokio::task::spawn_blocking`.
#[cfg(target_os = "windows")]
pub fn add_inbound_rule_elevated(channel: Channel, game_exe: &std::path::Path) -> Result<(), String> {
    let name_arg = format!("name={}", rule_name(channel));
    let program_arg = format!("program={}", game_exe.to_string_lossy());

    let status = runas::Command::new("netsh")
        // Don't flash a console window for the elevated child.
        .show(false)
        // runas::Command::args takes a slice (&[S]), not an owned array.
        .args(&[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            name_arg.as_str(),
            "dir=in",
            "action=allow",
            program_arg.as_str(),
            "enable=yes",
        ])
        .status()
        .map_err(|e| format!("failed to launch elevated netsh: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        // A non-zero exit covers a declined/cancelled UAC prompt as well as a
        // netsh failure — both mean no rule was created. Surface it so the
        // caller can keep the prompt open rather than pretend success.
        match status.code() {
            Some(code) => Err(format!(
                "couldn't create the firewall rule (netsh exit {code}) — the \
                 permission prompt may have been declined"
            )),
            None => Err("elevated netsh terminated without an exit code".to_string()),
        }
    }
}

/// Non-Windows stub — firewall rules are not the launcher's concern off Windows
/// (see module docs). Present so the call site in `app.rs` compiles everywhere.
#[cfg(not(target_os = "windows"))]
pub fn add_inbound_rule_elevated(_channel: Channel, _game_exe: &std::path::Path) -> Result<(), String> {
    Err("firewall rule creation is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_name_is_per_channel_and_locale_stable() {
        assert_eq!(rule_name(Channel::Stable), "BriskaBlast stable Game");
        assert_eq!(rule_name(Channel::Ea), "BriskaBlast ea Game");
        assert_eq!(rule_name(Channel::Dev), "BriskaBlast dev Game");
    }
}
