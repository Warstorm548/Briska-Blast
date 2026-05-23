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
    Settings { tab: SettingsTab },
    ChangeUsername { draft: String },
    LauncherUpdate,
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
                    state.identity.channels.insert(
                        channel,
                        ChannelCreds {
                            player_id: resp.player_id,
                            secret_token: resp.secret_token,
                        },
                    );
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
            state.identity.username = trimmed;
            state.welcome_draft.clear();

            // Persist the identity file BEFORE any server reach-out so the
            // file is present even if the launcher crashes between Confirm
            // and the first /register response landing.
            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(error = %e, "failed to save initial identity");
            }

            state.awaiting_username = false;
            return Task::batch(register_tasks(state));
        }
        Message::PlayPressed
        | Message::UpdatePressed
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
