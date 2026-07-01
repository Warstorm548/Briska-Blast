//! Change Username center view.
//! Matches mockup Example Imgs/UserNameChange.png.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, ZONE_GAP};
use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length};
use shared::protocol::messages::MAX_USERNAME_LEN;

pub fn view<'a>(state: &'a AppState, draft: &'a str) -> Element<'a, Message> {
    let trimmed_non_empty = !draft.trim().is_empty();

    let mut confirm = button(text("Confirm Change")).padding(8);
    if trimmed_non_empty {
        confirm = confirm.on_press(Message::ConfirmUsernameChange);
    }

    let form = column![
        text("Current UserName").size(14),
        container(text(state.identity.username.as_str()))
            .style(theme::bordered)
            .padding(8)
            .width(Length::Fixed(260.0)),
        text("New Username").size(14),
        text_input("Enter Name", draft)
            .on_input(Message::UsernameDraftChanged)
            .padding(8)
            .width(Length::Fixed(260.0)),
        // Live counter — the input is hard-capped at MAX_USERNAME_LEN, so this
        // shows the limit being reached rather than an error state.
        text(format!("{}/{}", draft.chars().count(), MAX_USERNAME_LEN)).size(12),
        confirm,
    ]
    .spacing(ZONE_GAP * 2)
    .align_x(Alignment::Center);

    let header = row![
        Space::new().width(Length::Fill),
        button(text("Close")).on_press(Message::CloseCenterMenu).padding(8),
    ];

    container(
        column![header, super::scroll_area(form)]
            .spacing(ZONE_GAP * 4)
            .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}
