//! Top-level Iced state machine.
//! AppState is the view-model the UI reads from; Message is the closed set
//! of things the UI can ask to happen.

use crate::channel::Channel;
use crate::identity::Identity;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::updater::{self, UpdateCheckOutcome};
use crate::{mock, ui};
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
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
    pub center_view: CenterView,
}

impl Default for AppState {
    fn default() -> Self {
        let identity = mock::mock_identity();
        let visible_channels = mock::VISIBLE_CHANNELS.to_vec();
        let mut server_reachable = BTreeMap::new();
        for c in &visible_channels {
            server_reachable.insert(*c, true);
        }
        Self {
            identity,
            selected_channel: Channel::Stable,
            visible_channels,
            server_reachable,
            branch_updates_available: mock::BRANCH_UPDATES_AVAILABLE.to_vec(),
            launcher_update_available: false,
            launcher_available_version: String::new(),
            launcher_release_notes: String::new(),
            update_check_in_flight: false,
            self_update_in_flight: false,
            last_self_update_error: None,
            game_running: false,
            center_view: CenterView::Default,
        }
    }
}

/// Iced boot — produces initial state and the first-launch GitHub Releases
/// query that populates `launcher_update_available` / `launcher_available_version`.
pub fn boot() -> (AppState, Task<Message>) {
    let state = AppState {
        update_check_in_flight: true,
        ..AppState::default()
    };
    (
        state,
        Task::perform(
            updater::check_for_update(),
            Message::LauncherUpdateCheckDone,
        ),
    )
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
                let trimmed = draft.trim();
                if !trimmed.is_empty() {
                    state.identity.username = trimmed.to_string();
                    state.center_view = CenterView::Default;
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
        Message::PlayPressed
        | Message::UpdatePressed
        | Message::UninstallChannel(_)
        | Message::VerifyChannel(_)
        | Message::GameSavePressed(_) => {}
    }
    Task::none()
}

pub fn view(state: &AppState) -> Element<'_, Message> {
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
