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
//! Per the P3 decision this module ships only the **non-elevated detection**
//! half: it reports whether an inbound rule for a given exe already exists,
//! with no admin rights required to look. The elevated `netsh … add rule`
//! write is intentionally stubbed (`add_inbound_rule_elevated`) pending
//! Jean-Luc's sign-off on option (A) — user-initiated, single UAC elevation.
//!
//! On Linux, outbound-initiated hole-punching traverses the default host
//! firewall (ufw doesn't block outbound; inbound for hole-punched flows is
//! solicited), so there is nothing to detect or manipulate. The detection
//! function returns `NotApplicable` there and no Linux firewall code exists
//! in this module by design.

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

/// TODO(P3/layer1): create the inbound allow rule for the game exe. This needs
/// a single UAC elevation and is blocked on Jean-Luc's sign-off for option (A)
/// (user-initiated, launcher-driven, one-time prompt — mirrors the project's
/// user-initiated-update philosophy). It deliberately returns `Err` so any
/// caller wired up before that sign-off cannot silently believe a rule was
/// created. The eventual implementation runs, elevated, the equivalent of:
///
///   netsh advfirewall firewall add rule name="BriskaBlast <channel> Game" \
///       dir=in action=allow program="<game_exe>" enable=yes
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn add_inbound_rule_elevated(_game_exe: &std::path::Path) -> Result<(), String> {
    Err("firewall rule creation not yet enabled (pending P3 option-A sign-off)".into())
}
