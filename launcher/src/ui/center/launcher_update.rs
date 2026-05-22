//! Launcher Update center view.
//! Triggered by the "Update available: launcher" banner in the top bar.
//! v1 stub: shows current vs available version; Start Update button logs only.
//! Real self_update plumbing is documented in
//! docs/launcher/launcher-update-and-version-validation.md and ships in a
//! follow-up branch.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    let current = env!("CARGO_PKG_VERSION");
    let available = state.launcher_available_version.as_str();

    let body = column![
        text(format!("Current version: {current}")).size(16),
        container(text(format!("New version available: {available}")))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(320.0))
            .center_x(Length::Fixed(320.0)),
        button(text("Start Update"))
            .on_press(Message::StartLauncherUpdatePressed)
            .padding(8),
    ]
    .spacing(ZONE_GAP * 3)
    .align_x(Alignment::Center);

    let header = row![
        text("Launcher Update").size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close")).on_press(Message::CloseCenterMenu).padding(8),
    ]
    .align_y(Alignment::Center);

    container(
        column![header, body]
            .spacing(ZONE_GAP * 4)
            .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}
