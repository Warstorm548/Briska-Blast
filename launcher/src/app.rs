//! Top-level Iced state machine.
//! AppState is the view-model the UI reads from; Message is the closed set
//! of things the UI can ask to happen. v1 only meaningfully handles
//! ChannelPicked; other messages log and no-op per the plan's acceptance.

use crate::channel::Channel;
use crate::identity::Identity;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::{mock, ui};
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Message {
    PlayPressed,
    UpdatePressed,
    OpenSettings,
    ChannelPicked(Channel),
    ChangeNamePressed,
    LauncherUpdatePressed,
}

pub struct AppState {
    pub identity: Identity,
    pub selected_channel: Channel,
    pub visible_channels: Vec<Channel>,
    pub server_reachable: BTreeMap<Channel, bool>,
    pub branch_updates_available: Vec<Channel>,
    pub launcher_update_available: bool,
    pub game_running: bool,
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
            game_running: false,
        }
    }
}

pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    tracing::debug!(?message, "ui message received");
    if let Message::ChannelPicked(c) = message {
        state.selected_channel = c;
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
