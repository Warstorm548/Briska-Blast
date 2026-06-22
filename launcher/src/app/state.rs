//! `AppState` — the view-model the UI reads from. Mutated by `super::update`
//! and the per-feature handlers; constructed by `super::boot`.

use super::message::CenterView;
use crate::channel::Channel;
use crate::identity::Identity;
use std::collections::{BTreeMap, HashSet};

pub struct AppState {
    pub identity: Identity,
    pub selected_channel: Channel,
    pub visible_channels: Vec<Channel>,
    pub server_reachable: BTreeMap<Channel, bool>,
    pub branch_updates_available: Vec<Channel>,
    pub launcher_update_available: bool,
    pub launcher_available_version: String,
    pub launcher_release_notes: String,
    pub update_check_in_flight: bool,
    pub self_update_in_flight: bool,
    pub last_self_update_error: Option<String>,
    pub game_running: bool,
    /// True when boot's liveness probe found a game already running — the
    /// launcher was closed and reopened while a game (it or a previous launcher
    /// spawned) is still open, so there is no in-memory child handle /
    /// `spawn_and_wait` task to drive `GameExited`. Switches on the poll
    /// subscription (`super::subscription`) that re-probes the game's
    /// `game_instance.json` socket and clears `game_running` once that process
    /// finally exits. `false` in the normal launch case. See `crate::rendezvous`.
    pub recovered_game_running: bool,
    /// Transient one-line notice shown under the name in the right-rail username
    /// box — currently used when the server rejected a username change and the
    /// launcher reverted to the stored value. Cleared when the user starts the
    /// next change. `None` = nothing to show.
    pub username_notice: Option<String>,
    /// True only when the dev server's /register response reports
    /// `dev_flag = true` for this user on the current launch. Never
    /// persisted — server is the source of truth (see foundation §3).
    pub dev_flag: bool,
    /// Set on boot when no username is on file. While true, `view()`
    /// renders the welcome screen instead of the main 5-zone layout and
    /// boot's /register fan-out is held back — the server's first record
    /// of this user must carry their chosen name, not a placeholder.
    pub awaiting_username: bool,
    /// Live text in the welcome screen's input field.
    pub welcome_draft: String,
    pub center_view: CenterView,
    /// Set while a per-channel install / update is downloading + extracting.
    /// Used to disable buttons that would conflict (Play, Update, channel
    /// switch) and to drive the "Installing…" UI state on the prompt.
    pub install_in_progress: Option<Channel>,
    /// Latest game version on GitHub per channel, populated by the per-launch
    /// `latest_release` fan-out (Stage 4). Absent entries mean either the
    /// fetch is still in flight, the channel has no release yet, or the
    /// fetch failed — the bottom-left button state machine handles all
    /// three by leaving its label disabled (`Install Game` greyed when no
    /// available + no installed; `Up to date — vX.Y.Z` when no available
    /// but an install is on disk).
    pub available_versions: BTreeMap<Channel, semver::Version>,
    /// Latest InstallProgress event from the active download / extract.
    /// Drives the bottom-bar progress widget (Stage 6). `None` between
    /// installs; cleared on InstallComplete.
    pub download_progress: Option<crate::updater::branches::InstallProgress>,
    /// Last Verify File Integrity outcome per channel (Stage 7). Drives
    /// the inline status cell in Settings → Game Channel Management. Not
    /// persisted — fresh on every launcher launch.
    pub verify_results: BTreeMap<Channel, crate::updater::branches::VerifyOutcome>,
    /// Set while a per-channel uninstall is running. Prevents a fast
    /// double-press of Confirm from spawning two destructive tasks
    /// against the same install dir.
    pub uninstall_in_progress: Option<Channel>,
    /// Last Windows-Firewall inbound-rule check per channel (P3). Drives the
    /// inline status cell in Settings → Game Channel Management. Detection is
    /// non-elevated and button-triggered; absent entries mean "not checked
    /// this launch". On Linux the check resolves to `NotApplicable`.
    pub firewall_status: BTreeMap<Channel, crate::firewall::FirewallStatus>,
    /// Channels for which the user dismissed the first-Play firewall prompt this
    /// session (chose "Skip & Play"). In-memory only — re-prompts on the next
    /// launcher restart while the rule is still missing. A successful add makes
    /// the rule detectable, so accepted channels never re-prompt regardless.
    pub firewall_prompt_dismissed: HashSet<Channel>,
    /// Last manual "Check for Updates" outcome per channel, driven by the
    /// left-rail button under the channel picker (`ui::left_rail`). Drives the
    /// verdict box shown for the focused channel. Not persisted; a channel
    /// switch (`nav::channel_picked`) drops completed verdicts so the box resets
    /// to the em-dash, but keeps any in-flight `Checking` sentinel.
    pub channel_update_status: BTreeMap<Channel, ChannelUpdateStatus>,
}

/// Result of a manual per-channel update check. The check refreshes
/// `available_versions` (which the bottom-bar button already reads), and this
/// records the user-facing verdict for the left-rail status box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelUpdateStatus {
    /// Fetch in flight — disables the button to block a double-press.
    Checking,
    /// Installed version is at or above the latest published release.
    UpToDate(semver::Version),
    /// A newer release than the installed version is available.
    UpdateAvailable(semver::Version),
    /// The GitHub fetch errored; prior `available_versions` is left intact.
    Failed,
    /// The rate-limit back-off gate is closed — no GitHub request was spent.
    /// `resume_at` is a preformatted local `HH:MM` for display.
    RateLimited { resume_at: String },
}

impl ChannelUpdateStatus {
    /// Classify a completed check from the installed vs. available versions.
    /// `UpdateAvailable` only when a release is known AND strictly newer than
    /// what's installed; everything else (equal, older, or no release) is
    /// `UpToDate`, reported against whichever version we can show.
    pub(crate) fn from_check(
        installed: Option<&semver::Version>,
        available: Option<&semver::Version>,
    ) -> Self {
        match (installed, available) {
            (Some(inst), Some(avail)) if avail > inst => {
                ChannelUpdateStatus::UpdateAvailable(avail.clone())
            }
            (Some(inst), _) => ChannelUpdateStatus::UpToDate(inst.clone()),
            (None, Some(avail)) => ChannelUpdateStatus::UpToDate(avail.clone()),
            (None, None) => ChannelUpdateStatus::Failed,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        // Default visibility per foundation §3 visibility matrix: Stable + EA
        // always visible; Dev hidden until the dev server's /register returns
        // dev_flag = true on this launch.
        let visible_channels = vec![Channel::Stable, Channel::Ea];
        Self {
            // Empty username is the sentinel that triggers the welcome
            // screen in `boot()` — keep it empty here, populated from the
            // loaded identity file or the welcome form's Confirm action.
            identity: Identity {
                username: String::new(),
                channels: BTreeMap::new(),
            },
            selected_channel: Channel::Stable,
            visible_channels,
            server_reachable: BTreeMap::new(),
            // Empty by default; populated by `recompute_branch_updates_available`
            // when LatestReleaseFetched events arrive (Stage 4 — was mocked
            // through 0.5.x).
            branch_updates_available: Vec::new(),
            launcher_update_available: false,
            launcher_available_version: String::new(),
            launcher_release_notes: String::new(),
            update_check_in_flight: false,
            self_update_in_flight: false,
            last_self_update_error: None,
            game_running: false,
            recovered_game_running: false,
            username_notice: None,
            dev_flag: false,
            awaiting_username: false,
            welcome_draft: String::new(),
            center_view: CenterView::Default,
            install_in_progress: None,
            available_versions: BTreeMap::new(),
            download_progress: None,
            verify_results: BTreeMap::new(),
            uninstall_in_progress: None,
            firewall_status: BTreeMap::new(),
            firewall_prompt_dismissed: HashSet::new(),
            channel_update_status: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChannelUpdateStatus;
    use semver::Version;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn newer_release_is_update_available() {
        let inst = v("0.12.1");
        let avail = v("0.13.0");
        assert_eq!(
            ChannelUpdateStatus::from_check(Some(&inst), Some(&avail)),
            ChannelUpdateStatus::UpdateAvailable(avail),
        );
    }

    #[test]
    fn equal_or_older_release_is_up_to_date() {
        let inst = v("0.13.0");
        // Equal → up to date, reported against the installed version.
        assert_eq!(
            ChannelUpdateStatus::from_check(Some(&inst), Some(&v("0.13.0"))),
            ChannelUpdateStatus::UpToDate(inst.clone()),
        );
        // Remote somehow older → still up to date.
        assert_eq!(
            ChannelUpdateStatus::from_check(Some(&inst), Some(&v("0.12.0"))),
            ChannelUpdateStatus::UpToDate(inst),
        );
    }

    #[test]
    fn no_release_with_install_is_up_to_date() {
        let inst = v("0.13.0");
        assert_eq!(
            ChannelUpdateStatus::from_check(Some(&inst), None),
            ChannelUpdateStatus::UpToDate(inst),
        );
    }

    /// A fresh launcher must not believe a game is running until the boot probe
    /// says so — the poll subscription is gated on this flag.
    #[test]
    fn recovered_game_running_defaults_false() {
        assert!(!super::AppState::default().recovered_game_running);
    }
}
