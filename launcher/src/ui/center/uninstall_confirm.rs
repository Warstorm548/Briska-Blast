//! Uninstall confirmation prompt for the per-channel game-files pipeline.
//!
//! Triggered by Settings → Game Channel Management → "Uninstall" on a row
//! that has an install on record. Shows channel, installed version, the
//! resolved install dir, the foundation §2 "Keep player data for future
//! reinstall?" radio, and Cancel / Uninstall buttons. The destructive
//! task runs via `updater::branches::uninstall_install` and the prompt
//! stays open with `error` populated if it fails.

use crate::app::{AppState, CenterView, Message};
use crate::channel::Channel;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, radio, row, text, Space};
use iced::{Alignment, Element, Length};
use std::path::Path;

pub fn view<'a>(
    state: &'a AppState,
    channel: Channel,
    install_dir: &'a Path,
    installed_version: &'a str,
    keep_saves: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let header = row![
        text(format!("Uninstall {} Game", channel.label())).size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
    ]
    .align_y(Alignment::Center);

    container(
        column![
            header,
            content(state, channel, install_dir, installed_version, keep_saves, error)
        ]
        .spacing(ZONE_GAP * 4)
        .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}

fn content<'a>(
    state: &'a AppState,
    channel: Channel,
    install_dir: &'a Path,
    installed_version: &'a str,
    keep_saves: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let summary = container(text(format!(
        "Currently installed: {} v{}",
        channel.label(),
        installed_version
    )))
    .style(theme::bordered)
    .padding(12)
    .width(Length::Fixed(460.0))
    .center_x(Length::Fixed(460.0));

    let path_row = container(text(install_dir.display().to_string()).size(13))
        .style(theme::bordered)
        .padding(10)
        .width(Length::Fixed(460.0));

    // Foundation §2 wording: "Keep player data for future reinstall?"
    let saves_question = text("Keep player data for future reinstall?").size(15);
    let keep_radio = radio(
        "Keep saves (recommended)",
        true,
        Some(keep_saves),
        Message::UninstallKeepSavesToggled,
    );
    let drop_radio = radio(
        "Wipe everything",
        false,
        Some(keep_saves),
        Message::UninstallKeepSavesToggled,
    );
    let radio_col = column![keep_radio, drop_radio]
        .spacing(ZONE_GAP)
        .align_x(Alignment::Start);

    // Confirm button is disabled while another install / uninstall might
    // already be in flight (defence-in-depth — the Settings buttons already
    // disable themselves in that case but the prompt could outlive a state
    // change).
    let can_uninstall = state.install_in_progress.is_none() && !state.game_running;
    let mut uninstall_btn = button(text("Uninstall")).padding(8);
    if can_uninstall {
        uninstall_btn = uninstall_btn.on_press(Message::UninstallConfirmed);
    }
    let buttons = row![
        button(text("Cancel"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
        uninstall_btn,
    ]
    .spacing(ZONE_GAP * 2);

    let status_line: Element<'_, Message> = if let Some(err) = error {
        text(err.to_string()).size(13).into()
    } else if keep_saves {
        text(
            "Saves at <install>/saves/ will be moved to a timestamped \
            .briska-saves-backup/ dir beside the install before deletion.",
        )
        .size(12)
        .into()
    } else {
        text("Saves at <install>/saves/ will be deleted along with the install.")
            .size(12)
            .into()
    };

    column![
        summary,
        path_row,
        saves_question,
        radio_col,
        buttons,
        status_line,
    ]
    .spacing(ZONE_GAP * 3)
    .align_x(Alignment::Center)
    .into()
}

/// Dispatch convenience matching the pattern used by install_prompt.rs.
pub fn dispatch<'a>(state: &'a AppState, view_state: &'a CenterView) -> Element<'a, Message> {
    if let CenterView::UninstallConfirm {
        channel,
        install_dir,
        installed_version,
        keep_saves,
        error,
    } = view_state
    {
        view(
            state,
            *channel,
            install_dir,
            installed_version,
            *keep_saves,
            error.as_deref(),
        )
    } else {
        container(text("")).into()
    }
}
