//! Top-level Iced state machine.
//! AppState is the view-model the UI reads from; Message is the closed set
//! of things the UI can ask to happen.

use crate::channel::Channel;
use crate::identity::Identity;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::{mock, ui};
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    ChannelManagement,
    Graphics,
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
}

pub struct AppState {
    pub identity: Identity,
    pub selected_channel: Channel,
    pub visible_channels: Vec<Channel>,
    pub server_reachable: BTreeMap<Channel, bool>,
    pub branch_updates_available: Vec<Channel>,
    pub launcher_update_available: bool,
    pub launcher_available_version: String,
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
            launcher_update_available: mock::LAUNCHER_UPDATE_AVAILABLE,
            launcher_available_version: mock::LAUNCHER_AVAILABLE_VERSION.to_string(),
            game_running: false,
            center_view: CenterView::Default,
        }
    }
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
        Message::PlayPressed
        | Message::UpdatePressed
        | Message::StartLauncherUpdatePressed
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
