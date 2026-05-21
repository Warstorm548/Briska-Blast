//! Zone 4: username + Change Name button + per-channel Player IDs.
//! Dev row hidden when not in `state.visible_channels`.

use crate::app::{AppState, Message};
use crate::ui::theme::{RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, column, text};
use iced::{Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    let mut col = column![
        text(state.identity.username.clone()).size(18),
        button(text("Change Name")).on_press(Message::ChangeNamePressed),
        text(""),
        text("Player IDs").size(14),
    ]
    .spacing(ZONE_GAP * 2)
    .padding(8)
    .width(Length::Fixed(RAIL_WIDTH as f32));

    for channel in &state.visible_channels {
        if let Some(creds) = state.identity.channels.get(channel) {
            col = col.push(text(format!(
                "{} #{}",
                channel.label(),
                creds.player_id
            )));
        }
    }

    col.into()
}
