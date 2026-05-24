//! Top-level Iced state machine.
//! AppState is the view-model the UI reads from; Message is the closed set
//! of things the UI can ask to happen.

use crate::channel::Channel;
use crate::identity::{self, ChannelCreds, Identity};
use crate::server_api;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::updater::{self, UpdateCheckOutcome};
use crate::{mock, ui};
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
use shared::protocol::messages::{RegisterRequest, RegisterResponse, UpdateUsernameRequest};
use std::collections::BTreeMap;

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
    #[allow(dead_code)]
    UninstallChannel(Channel),
    #[allow(dead_code)]
    VerifyChannel(Channel),
    #[allow(dead_code)]
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
            branch_updates_available: mock::BRANCH_UPDATES_AVAILABLE.to_vec(),
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

/// Iced boot — produces initial state and spawns:
///   1. the GitHub Releases self-update check (always runs — GitHub-only,
///      no identity needed)
///   2. one /register call per channel — but ONLY when a non-empty username
///      is already on file. First-launch users see the welcome screen and
///      `ConfirmWelcomeUsername` kicks the /register fan-out instead.
pub fn boot() -> (AppState, Task<Message>) {
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
        state.awaiting_username = true;
    } else {
        tasks.extend(register_tasks(&state));
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
            return Task::batch(register_tasks(state));
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
            let installed = state
                .identity
                .channels
                .get(&channel)
                .and_then(|c| c.install_location.as_ref())
                .is_some();
            if installed {
                // Stage 4 will handle update-when-installed. Stage 3 only
                // routes the "not installed" path.
                tracing::debug!(?channel, "channel already installed — Stage 4 path pending");
                return Task::none();
            }
            state.center_view = CenterView::InstallPrompt {
                channel,
                install_root: None,
                available: None,
                error: None,
            };
            return Task::perform(
                crate::updater::branches::latest_release(channel),
                move |result| Message::InstallPromptLatestFetched { channel, result },
            );
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
            if state.install_in_progress.is_some() {
                return Task::none();
            }
            state.install_in_progress = Some(channel);
            return Task::perform(
                async move {
                    let fresh = crate::updater::branches::latest_release(channel).await?;
                    let Some(release) = fresh else {
                        return Err("release disappeared from GitHub between check and install"
                            .to_string());
                    };
                    if release.version.to_string() != version {
                        tracing::warn!(
                            expected = %version,
                            actual = %release.version,
                            "release version changed between check and install"
                        );
                    }
                    crate::updater::branches::download_and_install(
                        channel,
                        release,
                        install_root,
                        |progress| tracing::debug!(?progress, "install progress"),
                    )
                    .await
                },
                move |result| Message::InstallComplete { channel, result },
            );
        }
        Message::InstallComplete { channel, result } => {
            state.install_in_progress = None;
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
        Message::PlayPressed
        | Message::UninstallChannel(_)
        | Message::VerifyChannel(_)
        | Message::GameSavePressed(_) => {}
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
