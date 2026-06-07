//! The closed set of things the UI can ask to happen (`Message`) plus the
//! center-panel view enum (`CenterView`) and settings-tab selector
//! (`SettingsTab`). These are the public vocabulary the `ui/*` modules read
//! and emit; behaviour lives in `super::update` and `super::handlers`.

use crate::channel::Channel;
use crate::server_api;
use crate::updater::UpdateCheckOutcome;
use shared::protocol::messages::RegisterResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    ChannelManagement,
    Graphics,
    LauncherOptions,
}

#[derive(Debug, Clone)]
pub enum CenterView {
    Default,
    Settings {
        tab: SettingsTab,
    },
    ChangeUsername {
        draft: String,
    },
    LauncherUpdate,
    /// Per-channel game install prompt — opened by clicking the bottom-left
    /// "Install <Channel> Game" button when that channel has no install on
    /// disk. The latest_release fetch is kicked off when the prompt opens
    /// and writes into `available`; the folder picker writes `install_root`.
    InstallPrompt {
        channel: Channel,
        install_root: Option<std::path::PathBuf>,
        available: Option<String>,
        error: Option<String>,
    },
    /// Per-channel uninstall confirmation (Stage 7). `keep_saves` reflects
    /// the user's choice in the radio; defaults to true (Foundation §2
    /// preference). `error` surfaces any failure from the destructive
    /// task without closing the prompt.
    UninstallConfirm {
        channel: Channel,
        install_dir: std::path::PathBuf,
        installed_version: String,
        keep_saves: bool,
        error: Option<String>,
    },
    /// First-Play firewall prompt (Windows). Shown when a Play is requested for
    /// a channel that has no inbound rule yet (option A). Carries the resolved
    /// `exe` (rule target + display) plus the `install_dir`/`username` needed to
    /// launch the game once the user picks Allow or Skip. `in_progress` is true
    /// while the elevated `netsh` task runs; `error` surfaces an add-rule
    /// failure without closing the prompt.
    FirewallPrompt {
        channel: Channel,
        exe: std::path::PathBuf,
        install_dir: std::path::PathBuf,
        username: String,
        in_progress: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Message {
    PlayPressed,
    UpdatePressed,
    OpenSettings,
    ChannelPicked(Channel),
    ChangeNamePressed,
    LauncherUpdatePressed,
    CloseCenterMenu,
    SettingsTabSelected(SettingsTab),
    UsernameDraftChanged(String),
    ConfirmUsernameChange,
    UninstallChannel(Channel),
    VerifyChannel(Channel),
    GameSavePressed(Channel),
    StartLauncherUpdatePressed,
    CheckForUpdatesPressed,
    LauncherUpdateCheckDone(Result<UpdateCheckOutcome, String>),
    SelfUpdateDone(Result<(), String>),
    RegisterDone {
        channel: Channel,
        result: Result<RegisterResponse, String>,
    },
    UpdateUsernameDone {
        channel: Channel,
        result: Result<(), server_api::ServerApiError>,
    },
    WelcomeDraftChanged(String),
    ConfirmWelcomeUsername,
    // ---- Stage 3: per-channel game install pipeline ----
    /// Open the OS-native folder picker for the install_prompt route.
    PickInstallLocation,
    /// Result of the folder picker; `None` if the user cancelled.
    InstallLocationPicked(Option<std::path::PathBuf>),
    /// `latest_release` task finished for the currently-open install_prompt.
    InstallPromptLatestFetched {
        channel: Channel,
        result: Result<Option<crate::updater::branches::GameRelease>, String>,
    },
    /// User confirmed the install prompt — kick off download_and_install.
    InstallConfirmed,
    /// download_and_install finished; updates identity.json on success.
    InstallComplete {
        channel: Channel,
        result: Result<crate::updater::branches::InstallResult, String>,
    },
    /// Per-chunk download / extract progress event from the installer
    /// pipeline (Stage 6). Drives the bottom-bar progress widget.
    DownloadProgress {
        channel: Channel,
        progress: crate::updater::branches::InstallProgress,
    },
    /// Boot-time per-channel `latest_release` query landed (Stage 4). Drives
    /// the bottom-left button's state machine and the top "Updates
    /// available" banner. One fires per visible channel; Dev's fires only
    /// after dev_flag returns true on the dev /register response.
    LatestReleaseFetched {
        channel: Channel,
        result: Result<Option<crate::updater::branches::GameRelease>, String>,
    },
    /// Game process exited (Stage 5). `result` is the exit code (or `Ok(None)`
    /// if the process was killed by a signal) on success, or a spawn /
    /// wait error string. Clears `game_running` so the launcher returns
    /// to its idle layout.
    GameExited {
        channel: Channel,
        result: Result<Option<i32>, String>,
    },
    // ---- Stage 7: uninstall / verify / game save ----
    /// Toggle the Keep-saves radio inside the uninstall confirmation prompt.
    UninstallKeepSavesToggled(bool),
    /// User confirmed the uninstall prompt — kick off the destructive task.
    UninstallConfirmed,
    /// uninstall_install finished; clears install_location / installed_version
    /// from the channel row of identity.json and persists.
    UninstallComplete {
        channel: Channel,
        result: Result<(), String>,
    },
    /// verify_install finished — outcome cached in state.verify_results for
    /// the inline status cell in Settings → Game Channel Management.
    VerifyComplete {
        channel: Channel,
        outcome: crate::updater::branches::VerifyOutcome,
    },
    /// Result of the platform `open` call for the Game Save button. Only
    /// logged for now; the button itself doesn't surface failures in the UI.
    GameSaveOpenDone {
        channel: Channel,
        result: Result<(), String>,
    },
    /// User asked to (re)check the Windows-Firewall inbound rule for a
    /// channel's installed game exe (P3). Non-elevated, read-only.
    CheckFirewall(Channel),
    /// Firewall check finished — status cached in state.firewall_status.
    FirewallCheckComplete {
        channel: Channel,
        status: crate::firewall::FirewallStatus,
    },
    /// Pre-launch (first-Play) firewall check resolved. If `status` is
    /// `NotDetected` and `exe` is known we open the FirewallPrompt; otherwise we
    /// launch directly. `install_dir`/`username` are threaded through so the
    /// launch can proceed without re-deriving them. Only constructed on the
    /// Windows Play path, so it reads as dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    PlayFirewallResolved {
        channel: Channel,
        status: crate::firewall::FirewallStatus,
        exe: Option<std::path::PathBuf>,
        install_dir: std::path::PathBuf,
        username: String,
    },
    /// User chose "Allow & Play" on the firewall prompt — run the elevated
    /// `netsh add rule` (single UAC), reading the target from the open prompt.
    FirewallPromptAllow,
    /// User chose "Skip & Play" — launch without a rule and suppress the prompt
    /// for this channel for the rest of the session.
    FirewallPromptSkip,
    /// Elevated add-rule task finished. On success we close the prompt and
    /// launch; on failure we keep it open with an error so the user can retry
    /// or skip.
    FirewallRuleAddDone {
        channel: Channel,
        result: Result<(), String>,
    },
}
