//! Zone 4: username + Change Name button + per-channel Player IDs.
//! Dev row hidden when not in `state.visible_channels`.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, column, container, text};
use iced::{Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    column![username_box(state), player_ids_box(state),]
        .spacing(ZONE_GAP)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .into()
}

fn username_box(state: &AppState) -> Element<'_, Message> {
    container(
        column![
            text(state.identity.username.clone()).size(18),
            button(text("Change Name")).on_press(Message::ChangeNamePressed),
        ]
        .spacing(ZONE_GAP * 2),
    )
    .style(theme::bordered)
    .padding(8)
    .width(Length::Fill)
    .into()
}

fn player_ids_box(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Player IDs").size(14)].spacing(ZONE_GAP);
    for channel in &state.visible_channels {
        if let Some(creds) = state.identity.channels.get(channel) {
            col = col.push(text(format!(
                "{} #{}",
                channel.label(),
                creds.player_id
            )));
        }
    }
    container(col)
        .style(theme::bordered)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
