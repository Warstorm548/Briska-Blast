//! Top-level Iced state machine.
//! AppState is the view-model the UI reads from; Message is the closed set
//! of things the UI can ask to happen.

use crate::channel::Channel;
use crate::identity::{self, ChannelCreds, Identity};
use crate::server_api;
use crate::ui;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::updater::{self, UpdateCheckOutcome};
use futures_util::StreamExt;
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
use shared::protocol::messages::{RegisterRequest, RegisterResponse, UpdateUsernameRequest};
use std::collections::BTreeMap;

/// Internal pipe between the spawned download task and the Iced stream
/// adapter — see the `InstallConfirmed` handler. Not surfaced as a
/// public Message because the variants here are mapped 1:1 onto two
/// existing user-facing Messages (DownloadProgress, InstallComplete).
enum InstallStreamEvent {
    Progress(crate::updater::branches::InstallProgress),
    Complete(Result<crate::updater::branches::InstallResult, String>),
}

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
        result: Result<(), String>,
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
}

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
        }
    }
}

fn recompute_visible_channels(state: &mut AppState) {
    let mut v = vec![Channel::Stable, Channel::Ea];
    if state.dev_flag {
        v.push(Channel::Dev);
    }
    state.visible_channels = v;
}

fn register_request_for(state: &AppState, channel: Channel) -> RegisterRequest {
    let creds = state.identity.channels.get(&channel);
    RegisterRequest {
        username: state.identity.username.clone(),
        prior_player_id: creds.map(|c| c.player_id.clone()),
        prior_secret_token: creds.map(|c| c.secret_token.clone()),
    }
}

fn register_tasks(state: &AppState) -> Vec<Task<Message>> {
    let mut tasks = Vec::with_capacity(3);
    for channel in Channel::all() {
        let req = register_request_for(state, channel);
        tasks.push(Task::perform(
            server_api::register(channel, req),
            move |result| Message::RegisterDone { channel, result },
        ));
    }
    tasks
}

/// Boot-time GitHub Releases fan-out. One `latest_release` task per
/// **currently-visible** channel — Dev is added separately by the
/// RegisterDone(Dev, Ok) handler once `dev_flag` flips true, so unflagged
/// users never reach the GitHub API for the dev channel (foundation §3).
fn latest_release_tasks(state: &AppState) -> Vec<Task<Message>> {
    state
        .visible_channels
        .iter()
        .copied()
        .map(|channel| {
            Task::perform(
                crate::updater::branches::latest_release(channel),
                move |result| Message::LatestReleaseFetched { channel, result },
            )
        })
        .collect()
}

/// Derive `state.branch_updates_available` from real installed vs.
/// available versions. Filtered to `visible_channels` so the dev channel
/// never leaks into the top-bar banner for an unflagged user (foundation
/// §3 banner-filtering rule). Called from every handler that mutates
/// available_versions, installed_version, or visible_channels.
fn recompute_branch_updates_available(state: &mut AppState) {
    let mut updates = Vec::new();
    for channel in &state.visible_channels {
        let Some(creds) = state.identity.channels.get(channel) else {
            continue;
        };
        let Some(installed) = creds.parsed_installed_version() else {
            continue;
        };
        let Some(available) = state.available_versions.get(channel) else {
            continue;
        };
        if available > &installed {
            updates.push(*channel);
        }
    }
    state.branch_updates_available = updates;
}

/// Iced boot — produces initial state and spawns:
///   1. the GitHub Releases self-update check (always runs — GitHub-only,
///      no identity needed)
///   2. one /register call per channel — but ONLY when a non-empty username
///      is already on file. First-launch users see the welcome screen and
///      `ConfirmWelcomeUsername` kicks the /register fan-out instead.
pub fn boot() -> (AppState, Task<Message>) {
    // Layer-1 migration: anyone upgrading from a pre-installer build kept their
    // identity + saves next to the binary. Pull them into the per-user data
    // root before the first read so the account survives the move. Idempotent
    // and non-clobbering — see paths::migrate_legacy_data_if_needed.
    match crate::paths::migrate_legacy_data_if_needed() {
        Ok(true) => tracing::info!("migrated legacy launcher data into per-user data dir"),
        Ok(false) => {}
        Err(e) => tracing::warn!(
            error = %e,
            "legacy data migration failed; continuing with per-user data dir"
        ),
    }

    let loaded = match identity::load() {
        Ok(Some(id)) => Some(id),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "identity load failed; falling back to first-run");
            None
        }
    };

    let mut state = AppState {
        update_check_in_flight: true,
        ..AppState::default()
    };
    if let Some(id) = loaded {
        state.identity = id;
    }

    let mut tasks: Vec<Task<Message>> = vec![Task::perform(
        updater::check_for_update(),
        Message::LauncherUpdateCheckDone,
    )];

    if state.identity.username.trim().is_empty() {
        // Gate the entire identity flow behind the welcome screen so the
        // server's first record of this user carries their chosen name.
        // The latest_release fan-out is held back too — we don't want to
        // populate available_versions before the user has seen the welcome
        // form, and Dev's fetch is gated on the dev_flag handshake anyway.
        state.awaiting_username = true;
    } else {
        tasks.extend(register_tasks(&state));
        tasks.extend(latest_release_tasks(&state));
    }

    (state, Task::batch(tasks))
}

pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    tracing::debug!(?message, "ui message received");
    match message {
        Message::ChannelPicked(c) => state.selected_channel = c,
        Message::OpenSettings => {
            state.center_view = CenterView::Settings {
                tab: SettingsTab::ChannelManagement,
            };
        }
        Message::ChangeNamePressed => {
            state.center_view = CenterView::ChangeUsername {
                draft: state.identity.username.clone(),
            };
        }
        Message::LauncherUpdatePressed => state.center_view = CenterView::LauncherUpdate,
        Message::CloseCenterMenu => state.center_view = CenterView::Default,
        Message::SettingsTabSelected(t) => {
            if let CenterView::Settings { tab } = &mut state.center_view {
                *tab = t;
            }
        }
        Message::UsernameDraftChanged(s) => {
            if let CenterView::ChangeUsername { draft } = &mut state.center_view {
                *draft = s;
            }
        }
        Message::ConfirmUsernameChange => {
            if let CenterView::ChangeUsername { draft } = &state.center_view {
                let trimmed = draft.trim().to_string();
                if !trimmed.is_empty() {
                    state.identity.username = trimmed.clone();
                    state.center_view = CenterView::Default;
                    if let Err(e) = identity::save(&state.identity) {
                        tracing::warn!(error = %e, "failed to persist identity after rename");
                    }
                    // Tell every channel server the launcher already has
                    // credentials for — fire-and-forget; failures are logged
                    // but don't block the UI.
                    let mut tasks: Vec<Task<Message>> = Vec::new();
                    for (channel, creds) in &state.identity.channels {
                        let req = UpdateUsernameRequest {
                            player_id: creds.player_id.clone(),
                            secret_token: creds.secret_token.clone(),
                            username: trimmed.clone(),
                        };
                        let ch = *channel;
                        tasks.push(Task::perform(
                            server_api::update_username(ch, req),
                            move |result| Message::UpdateUsernameDone { channel: ch, result },
                        ));
                    }
                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
            }
        }
        Message::CheckForUpdatesPressed => {
            if !state.update_check_in_flight && !state.self_update_in_flight {
                state.update_check_in_flight = true;
                state.last_self_update_error = None;
                return Task::perform(
                    updater::check_for_update(),
                    Message::LauncherUpdateCheckDone,
                );
            }
        }
        Message::LauncherUpdateCheckDone(result) => {
            state.update_check_in_flight = false;
            match result {
                Ok(UpdateCheckOutcome::Available { version, notes }) => {
                    state.launcher_update_available = true;
                    state.launcher_available_version = version;
                    state.launcher_release_notes = notes;
                }
                Ok(UpdateCheckOutcome::UpToDate) => {
                    state.launcher_update_available = false;
                    state.launcher_available_version.clear();
                    state.launcher_release_notes.clear();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "launcher update check failed");
                    state.last_self_update_error = Some(format!("Update check failed: {e}"));
                }
            }
        }
        Message::StartLauncherUpdatePressed => {
            if state.game_running {
                tracing::warn!("refusing self-update: game is running");
                state.last_self_update_error =
                    Some("Cannot update while the game is running.".into());
            } else if state.self_update_in_flight {
                tracing::debug!("self-update already in flight, ignoring");
            } else if state.update_check_in_flight {
                tracing::debug!("update check in flight, ignoring start press");
            } else if !state.launcher_update_available
                || state.launcher_available_version.is_empty()
            {
                tracing::debug!("no update available, ignoring start press");
            } else {
                state.self_update_in_flight = true;
                state.last_self_update_error = None;
                let version = state.launcher_available_version.clone();
                return Task::perform(updater::run_self_update(version), Message::SelfUpdateDone);
            }
        }
        Message::SelfUpdateDone(result) => {
            state.self_update_in_flight = false;
            match result {
                Ok(()) => {
                    // Binary on disk has been swapped. Exit so the next launch
                    // runs the new code; `self_update`'s rename-trick cleanup
                    // happens on that next launch.
                    tracing::info!("self-update succeeded — exiting for relaunch");
                    std::process::exit(0);
                }
                Err(e) => {
                    tracing::error!(error = %e, "self-update failed");
                    state.last_self_update_error = Some(format!("Update failed: {e}"));
                }
            }
        }
        Message::RegisterDone { channel, result } => {
            match result {
                Ok(resp) => {
                    // Preserve install_location / installed_version if this
                    // channel was already installed. /register only refreshes
                    // identity creds; it must not wipe Stage 3 install state.
                    let prior = state.identity.channels.remove(&channel);
                    let mut creds =
                        ChannelCreds::from_register(resp.player_id, resp.secret_token);
                    if let Some(p) = prior {
                        creds.install_location = p.install_location;
                        creds.installed_version = p.installed_version;
                    }
                    state.identity.channels.insert(channel, creds);
                    // Server is canonical for username; reflect any drift back
                    // into state.identity.
                    state.identity.username = resp.username;
                    state.server_reachable.insert(channel, true);

                    if let Err(e) = identity::save(&state.identity) {
                        tracing::warn!(
                            error = %e,
                            channel = %channel,
                            "failed to persist identity after register"
                        );
                    }

                    if channel == Channel::Dev {
                        state.dev_flag = resp.dev_flag;
                        recompute_visible_channels(state);
                        if resp.dev_flag {
                            // Dev just became visible — fetch its latest
                            // release now (boot's fan-out skipped Dev when
                            // visible_channels was Stable+Ea only).
                            recompute_branch_updates_available(state);
                            return Task::perform(
                                crate::updater::branches::latest_release(Channel::Dev),
                                |result| Message::LatestReleaseFetched {
                                    channel: Channel::Dev,
                                    result,
                                },
                            );
                        } else {
                            // Dev hidden — drop any cached version so the
                            // banner doesn't dangle from a prior flagged
                            // run; rebuild the derived banner.
                            state.available_versions.remove(&Channel::Dev);
                            recompute_branch_updates_available(state);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, channel = %channel, "register failed");
                    state.server_reachable.insert(channel, false);
                    if channel == Channel::Dev {
                        // Dev server unreachable → must hide dev row, per
                        // the foundation visibility matrix (row "No / — /
                        // (unknowable) / No").
                        state.dev_flag = false;
                        recompute_visible_channels(state);
                        state.available_versions.remove(&Channel::Dev);
                        recompute_branch_updates_available(state);
                    }
                }
            }
        }
        Message::UpdateUsernameDone { channel, result } => {
            if let Err(e) = result {
                tracing::warn!(error = %e, channel = %channel, "update_username failed");
            }
        }
        Message::WelcomeDraftChanged(s) => state.welcome_draft = s,
        Message::ConfirmWelcomeUsername => {
            let trimmed = state.welcome_draft.trim().to_string();
            if trimmed.is_empty() {
                // Defence in depth — the Confirm button is disabled when
                // empty, but `on_submit` (Enter key) routes here too.
                return Task::none();
            }

            // Build a candidate and persist BEFORE mutating shared state, so
            // a save failure leaves both the on-disk file and AppState in
            // their pre-Confirm form. The welcome screen stays up and the
            // typed text in welcome_draft is preserved so the user can retry
            // without retyping. We must NOT proceed to /register on a save
            // failure: otherwise the server would record an identity we have
            // no on-disk record of, and the next boot would issue a fresh
            // player_id (different from the one already on the server).
            let mut candidate = state.identity.clone();
            candidate.username = trimmed;
            if let Err(e) = identity::save(&candidate) {
                tracing::warn!(
                    error = %e,
                    "failed to save initial identity; staying on welcome screen"
                );
                return Task::none();
            }

            state.identity = candidate;
            state.welcome_draft.clear();
            state.awaiting_username = false;
            let mut tasks = register_tasks(state);
            tasks.extend(latest_release_tasks(state));
            return Task::batch(tasks);
        }
        Message::UpdatePressed => {
            let channel = state.selected_channel;
            // Defence-in-depth: the channel selector hides Dev when
            // !dev_flag, so this should be unreachable in practice. Logged
            // and refused if it ever fires.
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("UpdatePressed for Dev without dev_flag — refusing");
                return Task::none();
            }
            if state.install_in_progress.is_some() {
                tracing::debug!(
                    in_progress = ?state.install_in_progress,
                    "install already in flight — ignoring"
                );
                return Task::none();
            }
            // Resolve the two pieces of state the button label is also
            // driven by, so the action taken matches what the user clicked.
            let creds = state.identity.channels.get(&channel);
            let installed_version = creds.and_then(|c| c.parsed_installed_version());
            let install_root_prior =
                creds.and_then(|c| c.install_location.as_ref().map(|p| {
                    // The install_location stored in identity.json is the
                    // resolved <root>/<channel>/ path; the prompt re-joins
                    // the channel dir, so we strip it back to the root.
                    p.parent().map(|pp| pp.to_path_buf()).unwrap_or_else(|| p.clone())
                }));
            let available_str = state
                .available_versions
                .get(&channel)
                .map(|v| v.to_string());
            // Decide what action to take. Mirrors the bottom-left button
            // state machine — the button is disabled in every "no action"
            // branch, so reaching them here means the state changed under
            // us (e.g. a late LatestReleaseFetched).
            let (install_root, action_label): (Option<std::path::PathBuf>, &'static str) =
                match (installed_version.as_ref(), state.available_versions.get(&channel)) {
                    // Update flow — same install_root, new version.
                    (Some(inst), Some(avail)) if avail > inst => {
                        (install_root_prior, "update")
                    }
                    // Fresh install — user picks the install_root next.
                    (None, Some(_)) => (None, "install"),
                    _ => {
                        tracing::debug!(
                            ?channel,
                            installed = ?installed_version,
                            available = ?available_str,
                            "UpdatePressed with no actionable state — ignoring"
                        );
                        return Task::none();
                    }
                };
            tracing::debug!(?channel, action = %action_label, "opening install prompt");
            state.center_view = CenterView::InstallPrompt {
                channel,
                install_root,
                available: available_str.clone(),
                error: None,
            };
            // Cache hit is the normal case; fall back to an in-prompt fetch
            // only when the cache is empty (rare — button is disabled then).
            if available_str.is_none() {
                return Task::perform(
                    crate::updater::branches::latest_release(channel),
                    move |result| Message::InstallPromptLatestFetched { channel, result },
                );
            }
        }
        Message::InstallPromptLatestFetched { channel, result } => {
            if let CenterView::InstallPrompt {
                channel: pc,
                available,
                error,
                ..
            } = &mut state.center_view
            {
                // Guard against a late arrival after the user navigated away
                // or switched channels — only apply when the prompt is still
                // on the same channel.
                if *pc != channel {
                    return Task::none();
                }
                match result {
                    Ok(Some(release)) => {
                        *available = Some(release.version.to_string());
                        *error = None;
                    }
                    Ok(None) => {
                        *error = Some(
                            "No game release published for this channel yet.".to_string(),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, ?channel, "latest_release fetch failed");
                        *error = Some(format!("Could not reach GitHub Releases: {e}"));
                    }
                }
            }
        }
        Message::PickInstallLocation => {
            return Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Choose game install location")
                        .pick_folder()
                        .await
                        .map(|fh| fh.path().to_path_buf())
                },
                Message::InstallLocationPicked,
            );
        }
        Message::InstallLocationPicked(picked) => {
            if let Some(path) = picked {
                if let CenterView::InstallPrompt { install_root, .. } = &mut state.center_view {
                    *install_root = Some(path);
                }
            }
        }
        Message::InstallConfirmed => {
            // Snapshot the prompt state — we need owned values for the async
            // task. If any piece is missing the Confirm button shouldn't be
            // pressable; logged + ignored as defence-in-depth.
            let (channel, install_root, version) = if let CenterView::InstallPrompt {
                channel,
                install_root: Some(root),
                available: Some(v),
                ..
            } = &state.center_view
            {
                (*channel, root.clone(), v.clone())
            } else {
                tracing::warn!("InstallConfirmed with incomplete prompt state");
                return Task::none();
            };
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("InstallConfirmed for Dev without dev_flag — refusing");
                return Task::none();
            }
            // Refuse the install if /register never produced creds for this
            // channel — InstallComplete's identity-update step would silently
            // no-op (channels.get_mut returns None), leaving install metadata
            // unpersisted and orphaning the on-disk files.
            if !state.identity.channels.contains_key(&channel) {
                tracing::warn!(
                    ?channel,
                    "InstallConfirmed for {channel} with missing credentials — refusing"
                );
                return Task::none();
            }
            if state.install_in_progress.is_some() {
                return Task::none();
            }
            // Parse the expected version up front so the staleness check
            // inside the async task compares Version-to-Version (handles
            // canonical-form differences like `1.2.3-dev.1` vs an equivalent
            // non-canonical string) instead of doing string equality. A
            // parse failure here is an upstream contract bug — log loudly
            // but proceed; the downstream installer is the safety net.
            let expected_version = match semver::Version::parse(&version) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        version,
                        "expected version from install prompt is not valid semver"
                    );
                    None
                }
            };
            state.install_in_progress = Some(channel);
            state.download_progress = None;
            // Drive the install with two channels: a tokio::spawn task that
            // owns the download (and produces Progress events into an mpsc
            // sender as it streams), and a Task::stream that consumes the
            // matching receiver and emits Messages into the Iced runtime.
            // The same Sender is cloned for the closure callback; the
            // outer clone fires the final Complete event once the .await
            // on download_and_install resolves.
            //
            // Bounded channel — Progress has latest-state semantics so we
            // try_send and drop on full (the next chunk's event arrives
            // shortly anyway). Complete is one-shot and must not be
            // dropped, so it uses the awaited send. Capacity 32 covers
            // ~half a second of progress events at a 64ms Iced frame and
            // a multi-MB/s download.
            const INSTALL_STREAM_CAPACITY: usize = 32;
            let (tx, rx) =
                tokio::sync::mpsc::channel::<InstallStreamEvent>(INSTALL_STREAM_CAPACITY);
            let tx_progress = tx.clone();
            tokio::spawn(async move {
                let result: Result<crate::updater::branches::InstallResult, String> = async {
                    let fresh = crate::updater::branches::latest_release(channel).await?;
                    let Some(release) = fresh else {
                        return Err(
                            "release disappeared from GitHub between check and install"
                                .to_string(),
                        );
                    };
                    if let Some(expected) = expected_version.as_ref() {
                        if &release.version != expected {
                            tracing::warn!(
                                expected = %expected,
                                actual = %release.version,
                                "release version changed between check and install"
                            );
                        }
                    }
                    crate::updater::branches::download_and_install(
                        channel,
                        release,
                        install_root,
                        move |progress| {
                            // Drop on full / receiver-dropped — progress is
                            // safe to skip (the next event corrects the
                            // displayed fraction); blocking the callback
                            // would stall the actual download loop.
                            let _ =
                                tx_progress.try_send(InstallStreamEvent::Progress(progress));
                        },
                    )
                    .await
                }
                .await;
                // Completion must not be dropped — `send().await` waits
                // for capacity. If the receiver has been dropped we don't
                // care (no UI to update); the `let _ =` absorbs that.
                let _ = tx.send(InstallStreamEvent::Complete(result)).await;
            });
            let stream =
                tokio_stream::wrappers::ReceiverStream::new(rx).map(move |ev| match ev {
                    InstallStreamEvent::Progress(progress) => {
                        Message::DownloadProgress { channel, progress }
                    }
                    InstallStreamEvent::Complete(result) => {
                        Message::InstallComplete { channel, result }
                    }
                });
            return Task::stream(stream);
        }
        Message::DownloadProgress { channel, progress } => {
            // Late events for a stale channel can arrive if the user
            // cancels and starts another install before the previous
            // stream drains. Discard those.
            if state.install_in_progress != Some(channel) {
                return Task::none();
            }
            state.download_progress = Some(progress);
        }
        Message::InstallComplete { channel, result } => {
            state.install_in_progress = None;
            state.download_progress = None;
            match result {
                Ok(info) => {
                    if let Some(creds) = state.identity.channels.get_mut(&channel) {
                        creds.install_location = Some(info.install_dir.clone());
                        creds.installed_version = Some(info.version.clone());
                    }
                    if let Err(e) = identity::save(&state.identity) {
                        tracing::warn!(
                            error = %e,
                            ?channel,
                            "failed to persist identity after install"
                        );
                    }
                    tracing::info!(
                        ?channel,
                        version = %info.version,
                        exe = %info.executable,
                        install_dir = %info.install_dir.display(),
                        "game install complete"
                    );
                    // installed_version just changed — refresh the derived
                    // top-bar banner so this channel falls out of the
                    // "Updates available" list.
                    recompute_branch_updates_available(state);
                    state.center_view = CenterView::Default;
                }
                Err(e) => {
                    tracing::warn!(error = %e, ?channel, "game install failed");
                    if let CenterView::InstallPrompt {
                        channel: pc,
                        error: prompt_error,
                        ..
                    } = &mut state.center_view
                    {
                        if *pc == channel {
                            *prompt_error = Some(format!("Install failed: {e}"));
                        }
                    }
                }
            }
        }
        Message::LatestReleaseFetched { channel, result } => {
            // Dev gating defence-in-depth: a late-arriving Dev fetch when
            // the user is no longer dev-flagged must not poison the cache.
            // (latest_release_tasks only spawns for visible channels, but a
            // request issued before the flag was revoked could still
            // resolve after.)
            if channel == Channel::Dev && !state.dev_flag {
                tracing::debug!("dropping late LatestReleaseFetched(Dev) — dev_flag is false");
                return Task::none();
            }
            match result {
                Ok(Some(release)) => {
                    state.available_versions.insert(channel, release.version);
                }
                Ok(None) => {
                    state.available_versions.remove(&channel);
                    tracing::info!(
                        ?channel,
                        "no game release published for this channel yet"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, ?channel, "latest_release fetch failed");
                    // Leave any prior cached version in place — the user
                    // still sees something while GitHub recovers.
                }
            }
            recompute_branch_updates_available(state);
        }
        Message::PlayPressed => {
            let channel = state.selected_channel;
            if state.game_running {
                tracing::debug!("game already running — ignoring PlayPressed");
                return Task::none();
            }
            if state.install_in_progress.is_some() {
                tracing::debug!(
                    in_progress = ?state.install_in_progress,
                    "install in flight — refusing to launch game"
                );
                return Task::none();
            }
            // Defence-in-depth dev gate. The channel selector hides Dev
            // when !dev_flag, but a stale UI state could still route here.
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("PlayPressed for Dev without dev_flag — refusing");
                return Task::none();
            }
            let Some(creds) = state.identity.channels.get(&channel) else {
                tracing::warn!(?channel, "PlayPressed with no creds for channel — refusing");
                return Task::none();
            };
            // Same half-state guard as the button label uses: only proceed
            // when both install_location AND a parseable installed_version
            // are present.
            if creds.parsed_installed_version().is_none() {
                tracing::warn!(?channel, "PlayPressed without a complete install — refusing");
                return Task::none();
            }
            let Some(install_dir) = creds.install_location.clone() else {
                // Unreachable given parsed_installed_version succeeded, but
                // keep the explicit match so a future schema change can't
                // sneak past.
                return Task::none();
            };
            let username = state.identity.username.clone();
            state.game_running = true;
            return Task::perform(
                crate::game_launch::spawn_and_wait(channel, install_dir, username),
                move |result| Message::GameExited { channel, result },
            );
        }
        Message::GameExited { channel, result } => {
            state.game_running = false;
            match result {
                Ok(Some(code)) => tracing::info!(?channel, code, "game exited"),
                Ok(None) => tracing::info!(?channel, "game exited (signal)"),
                Err(e) => tracing::warn!(?channel, error = %e, "game spawn / wait failed"),
            }
        }
        Message::UninstallChannel(channel) => {
            // Per Stage 7 design: Uninstall is allowed regardless of
            // dev_flag so a previously-flagged user who got revoked can
            // still clean up orphan dev files. We do still need a
            // complete install row to act on, and we won't proceed while
            // an install is in flight, an uninstall is already running,
            // or the game is running.
            if state.install_in_progress.is_some()
                || state.uninstall_in_progress.is_some()
                || state.game_running
            {
                tracing::debug!(?channel, "UninstallChannel refused — busy");
                return Task::none();
            }
            let Some(creds) = state.identity.channels.get(&channel) else {
                tracing::warn!(?channel, "UninstallChannel with no creds row — refusing");
                return Task::none();
            };
            let (Some(install_dir), Some(installed_version)) =
                (creds.install_location.clone(), creds.installed_version.clone())
            else {
                tracing::debug!(?channel, "UninstallChannel with no install on record — no-op");
                return Task::none();
            };
            state.center_view = CenterView::UninstallConfirm {
                channel,
                install_dir,
                installed_version,
                // Default Keep saves = true per foundation §2 — safer
                // default; the user has to explicitly opt into wiping.
                keep_saves: true,
                error: None,
            };
        }
        Message::UninstallKeepSavesToggled(v) => {
            if let CenterView::UninstallConfirm { keep_saves, .. } = &mut state.center_view {
                *keep_saves = v;
            }
        }
        Message::UninstallConfirmed => {
            let (channel, install_dir, keep_saves) = if let CenterView::UninstallConfirm {
                channel,
                install_dir,
                keep_saves,
                ..
            } = &state.center_view
            {
                (*channel, install_dir.clone(), *keep_saves)
            } else {
                tracing::warn!("UninstallConfirmed without an active UninstallConfirm view");
                return Task::none();
            };
            // Re-check the busy gate on Confirm too — defence-in-depth
            // against a Confirm press that races a state change after
            // the prompt was opened.
            if state.install_in_progress.is_some()
                || state.uninstall_in_progress.is_some()
                || state.game_running
            {
                return Task::none();
            }
            // Record the in-flight uninstall BEFORE returning the task so
            // a fast second Confirm press (or any other Uninstall
            // affordance) is rejected by the busy gates above and in the
            // UI. Cleared by UninstallComplete.
            state.uninstall_in_progress = Some(channel);
            tracing::info!(?channel, keep_saves, "starting uninstall");
            return Task::perform(
                crate::updater::branches::uninstall_install(
                    install_dir,
                    channel.dir_name(),
                    keep_saves,
                ),
                move |result| Message::UninstallComplete { channel, result },
            );
        }
        Message::UninstallComplete { channel, result } => {
            // Clear the in-flight flag unconditionally — success and
            // failure both end the spawned task.
            if state.uninstall_in_progress == Some(channel) {
                state.uninstall_in_progress = None;
            }
            match result {
            Ok(()) => {
                if let Some(creds) = state.identity.channels.get_mut(&channel) {
                    creds.install_location = None;
                    creds.installed_version = None;
                }
                if let Err(e) = identity::save(&state.identity) {
                    tracing::warn!(
                        error = %e,
                        ?channel,
                        "failed to persist identity after uninstall"
                    );
                }
                state.verify_results.remove(&channel);
                recompute_branch_updates_available(state);
                state.center_view = CenterView::Default;
                tracing::info!(?channel, "uninstall complete");
            }
            Err(e) => {
                tracing::warn!(error = %e, ?channel, "uninstall failed");
                if let CenterView::UninstallConfirm {
                    channel: pc,
                    error,
                    ..
                } = &mut state.center_view
                {
                    if *pc == channel {
                        *error = Some(format!("Uninstall failed: {e}"));
                    }
                }
            }
            }
        }
        Message::VerifyChannel(channel) => {
            // Defence-in-depth dev gate.
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("VerifyChannel for Dev without dev_flag — refusing");
                return Task::none();
            }
            let Some(creds) = state.identity.channels.get(&channel) else {
                return Task::none();
            };
            let Some(install_dir) = creds.install_location.clone() else {
                tracing::debug!(?channel, "VerifyChannel with no install — no-op");
                return Task::none();
            };
            return Task::perform(
                crate::updater::branches::verify_install(install_dir),
                move |outcome| Message::VerifyComplete { channel, outcome },
            );
        }
        Message::VerifyComplete { channel, outcome } => {
            tracing::info!(?channel, ?outcome, "verify complete");
            state.verify_results.insert(channel, outcome);
        }
        Message::GameSavePressed(channel) => {
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("GameSavePressed for Dev without dev_flag — refusing");
                return Task::none();
            }
            let Some(creds) = state.identity.channels.get(&channel) else {
                return Task::none();
            };
            let Some(install_dir) = creds.install_location.clone() else {
                tracing::debug!(?channel, "GameSavePressed with no install — no-op");
                return Task::none();
            };
            let saves_dir = install_dir.join("saves");
            return Task::perform(
                async move {
                    tokio::fs::create_dir_all(&saves_dir)
                        .await
                        .map_err(|e| format!("create saves dir: {e}"))?;
                    let to_open = saves_dir.clone();
                    tokio::task::spawn_blocking(move || open::that(&to_open))
                        .await
                        .map_err(|e| format!("open join: {e}"))?
                        .map_err(|e| format!("open: {e}"))
                },
                move |result| Message::GameSaveOpenDone { channel, result },
            );
        }
        Message::GameSaveOpenDone { channel, result } => match result {
            Ok(()) => tracing::info!(?channel, "saves dir opened"),
            Err(e) => tracing::warn!(?channel, error = %e, "failed to open saves dir"),
        },
        Message::CheckFirewall(channel) => {
            // Defence-in-depth dev gate, matching VerifyChannel.
            if channel == Channel::Dev && !state.dev_flag {
                tracing::warn!("CheckFirewall for Dev without dev_flag — refusing");
                return Task::none();
            }
            let Some(creds) = state.identity.channels.get(&channel) else {
                return Task::none();
            };
            let Some(install_dir) = creds.install_location.clone() else {
                tracing::debug!(?channel, "CheckFirewall with no install — no-op");
                return Task::none();
            };
            // Resolve the exact game exe (install_dir + manifest.executable),
            // then run the non-elevated firewall lookup against it. If the
            // manifest can't be read the rule target is unknown, so we surface
            // that as `Unknown` rather than guessing a path.
            return Task::perform(
                async move {
                    match crate::updater::branches::installed_manifest(&install_dir).await {
                        Ok(Some(m)) => {
                            let exe = install_dir.join(&m.executable);
                            crate::firewall::inbound_rule_status(exe).await
                        }
                        Ok(None) => crate::firewall::FirewallStatus::Unknown(
                            "no installed.json — channel not installed".to_string(),
                        ),
                        Err(e) => crate::firewall::FirewallStatus::Unknown(e),
                    }
                },
                move |status| Message::FirewallCheckComplete { channel, status },
            );
        }
        Message::FirewallCheckComplete { channel, status } => {
            tracing::info!(?channel, ?status, "firewall check complete");
            state.firewall_status.insert(channel, status);
        }
    }
    Task::none()
}

pub fn view(state: &AppState) -> Element<'_, Message> {
    if state.awaiting_username {
        return ui::welcome::view(state);
    }
    column![
        container(ui::top_bar::view(state)).height(Length::Fixed(BAR_HEIGHT as f32)),
        row![
            container(ui::left_rail::view(state)),
            container(ui::center::view(state)).width(Length::Fill),
            container(ui::right_rail::view(state)),
        ]
        .height(Length::Fill)
        .spacing(ZONE_GAP),
        container(ui::bottom_bar::view(state)).height(Length::Fixed(BAR_HEIGHT as f32)),
    ]
    .spacing(ZONE_GAP)
    .into()
}

pub fn theme(_state: &AppState) -> Theme {
    Theme::Dark
}

pub fn title(_state: &AppState) -> String {
    String::from("BriskaBlast Launcher")
}
