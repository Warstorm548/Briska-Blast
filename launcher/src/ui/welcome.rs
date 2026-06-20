//! First-launch welcome screen. Shown only when the launcher boots with no
//! username on file. The /register fan-out is gated behind it so the server's
//! first record of this user carries their chosen name rather than a
//! placeholder, and the identity file is written before any reach-out — even
//! a crash between "Confirm" and the first /register leaves a usable file on
//! disk so the next boot doesn't re-prompt.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, text, text_input};
use iced::{Alignment, Element, Length};
use shared::protocol::messages::MAX_USERNAME_LEN;

pub fn view(state: &AppState) -> Element<'_, Message> {
    let trimmed_non_empty = !state.welcome_draft.trim().is_empty();

    let mut confirm = button(text("Confirm").size(16)).padding(10);
    if trimmed_non_empty {
        confirm = confirm.on_press(Message::ConfirmWelcomeUsername);
    }

    let form = column![
        text("Welcome to Briska Blast").size(TITLE_SIZE),
        text("Pick a username to get started.").size(14),
        text_input("Enter Username", &state.welcome_draft)
            .on_input(Message::WelcomeDraftChanged)
            .on_submit(Message::ConfirmWelcomeUsername)
            .padding(10)
            .width(Length::Fixed(320.0)),
        // Live counter — the input is hard-capped at MAX_USERNAME_LEN.
        text(format!("{}/{}", state.welcome_draft.chars().count(), MAX_USERNAME_LEN)).size(12),
        confirm,
    ]
    .spacing(ZONE_GAP * 3)
    .align_x(Alignment::Center);

    container(form)
        .style(theme::menu_pane)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(48)
        .into()
}
