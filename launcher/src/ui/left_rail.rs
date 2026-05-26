//! Zone 2: channel picker on top, server-status dots below.
//! Dev row is filtered out for unflagged users via `state.visible_channels`.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{column, container, pick_list, text};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    column![channel_box(state), server_status_box(state),]
        .spacing(ZONE_GAP)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .into()
}

fn channel_box(state: &AppState) -> Element<'_, Message> {
    // While the game is running we render the channel as a static label
    // instead of the interactive picker — switching channels mid-session
    // would race the spawned process against the channel-scoped install
    // dir (foundation §5E). Restored to a pick_list as soon as the game
    // exits.
    let picker: Element<'_, Message> = if state.game_running {
        container(text(state.selected_channel.label()).size(16))
            .style(theme::bordered)
            .padding(8)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .into()
    } else {
        pick_list(
            state.visible_channels.clone(),
            Some(state.selected_channel),
            Message::ChannelPicked,
        )
        .width(Length::Fill)
        .into()
    };
    container(
        column![text("Channel").size(14), picker].spacing(ZONE_GAP * 2),
    )
    .style(theme::bordered)
    .padding(8)
    .width(Length::Fill)
    .into()
}

fn server_status_box(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Server status").size(14)].spacing(ZONE_GAP);
    for channel in &state.visible_channels {
        let reachable = state
            .server_reachable
            .get(channel)
            .copied()
            .unwrap_or(false);
        let dot = if reachable { "\u{25CF}" } else { "\u{25CB}" };
        col = col.push(text(format!("{}  {}", dot, channel.label())));
    }
    container(col)
        .style(theme::bordered)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
