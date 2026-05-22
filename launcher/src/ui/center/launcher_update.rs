//! Launcher Update center view.
//! Triggered by the "Update available: launcher" banner in the top bar.
//! Shows the current vs. available version, a "Check for Updates" button that
//! re-runs the GitHub Releases query, and a "Start Update" button that kicks
//! off the `self_update` rename-trick binary swap. Both buttons reflect
//! in-flight state and the game-running safety gate; any error from the
//! updater surfaces inline below the buttons.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    let current = env!("CARGO_PKG_VERSION");
    let available = state.launcher_available_version.as_str();

    let availability_cell: Element<'_, Message> = if state.launcher_update_available
        && !available.is_empty()
    {
        container(text(format!("New version available: {available}")))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(360.0))
            .center_x(Length::Fixed(360.0))
            .into()
    } else if state.update_check_in_flight {
        container(text("Checking for updates…"))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(360.0))
            .center_x(Length::Fixed(360.0))
            .into()
    } else {
        container(text("You are running the latest version."))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(360.0))
            .center_x(Length::Fixed(360.0))
            .into()
    };

    // Check-for-updates: disabled while a check OR a swap is in flight.
    let mut check_btn = button(text("Check for Updates")).padding(8);
    if !state.update_check_in_flight && !state.self_update_in_flight {
        check_btn = check_btn.on_press(Message::CheckForUpdatesPressed);
    }

    // Start Update: enabled only when an update is known to be available,
    // the game is not running, and no other operation is in flight.
    let can_start = state.launcher_update_available
        && !state.launcher_available_version.is_empty()
        && !state.game_running
        && !state.self_update_in_flight
        && !state.update_check_in_flight;
    let mut start_btn = button(text(if state.self_update_in_flight {
        "Updating…"
    } else {
        "Start Update"
    }))
    .padding(8);
    if can_start {
        start_btn = start_btn.on_press(Message::StartLauncherUpdatePressed);
    }

    let buttons = row![check_btn, start_btn].spacing(ZONE_GAP * 2);

    let status_line: Element<'_, Message> = if let Some(err) = state.last_self_update_error.as_ref()
    {
        text(err.clone()).size(14).into()
    } else if state.game_running {
        text("Cannot update while the game is running.")
            .size(14)
            .into()
    } else if !state.launcher_release_notes.is_empty() {
        let preview: String = state.launcher_release_notes.chars().take(220).collect();
        text(preview).size(13).into()
    } else {
        Space::new().height(Length::Fixed(1.0)).into()
    };

    let body = column![
        text(format!("Current version: {current}")).size(16),
        availability_cell,
        buttons,
        status_line,
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
