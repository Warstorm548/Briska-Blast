//! Install location prompt for the per-channel game-files pipeline.
//!
//! Triggered by clicking the bottom-left "Install <Channel> Game" button on
//! a channel that has no install yet. Lets the user pick a root directory
//! (game lands at `<picked_root>/<channel>/`), then kicks off the download.
//!
//! State for this view lives on `CenterView::InstallPrompt`; the latest
//! release is fetched once on entry (Stage 3 — Stage 4 will share a cached
//! boot-time result instead).

use crate::app::{AppState, CenterView, Message};
use crate::channel::Channel;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

pub fn view<'a>(
    state: &'a AppState,
    channel: Channel,
    install_root: Option<&'a PathBuf>,
    available: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let header = row![
        text(format!("Install {} Game", channel.label())).size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Cancel")).on_press(Message::CloseCenterMenu).padding(8),
    ]
    .align_y(Alignment::Center);

    container(
        column![header, content(state, channel, install_root, available, error)]
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
    install_root: Option<&'a PathBuf>,
    available: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let version_cell = match available {
        Some(v) => container(text(format!("Latest version: v{v}")))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(420.0))
            .center_x(Length::Fixed(420.0)),
        None if error.is_some() => container(text("Could not reach GitHub Releases."))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(420.0))
            .center_x(Length::Fixed(420.0)),
        None => container(text("Checking for latest release…"))
            .style(theme::bordered)
            .padding(12)
            .width(Length::Fixed(420.0))
            .center_x(Length::Fixed(420.0)),
    };

    let path_display = match install_root {
        Some(p) => p.join(channel.dir_name()).to_string_lossy().into_owned(),
        None => "(no location chosen)".to_string(),
    };

    // Whether this channel is already installed on disk. When it is, the prompt
    // is an *update* (the path is pre-seeded with the existing install root by
    // `update_pressed`), so the location must stay locked — re-pointing an update
    // at a different folder would leave the old install behind / corrupt the
    // update. We key off `parsed_installed_version()` (true install state) rather
    // than `install_root.is_some()`, which is also `Some` once a fresh-install
    // user picks a folder. On uninstall both creds are cleared, so this flips
    // back to false and "Choose…" re-enables for a new location.
    let is_update = state
        .identity
        .channels
        .get(&channel)
        .and_then(|c| c.parsed_installed_version())
        .is_some();

    let mut choose_btn = button(text("Choose…")).padding(8);
    if !is_update {
        choose_btn = choose_btn.on_press(Message::PickInstallLocation);
    }

    let path_row = row![
        container(text(path_display).size(14))
            .style(theme::bordered)
            .padding(10)
            .width(Length::Fixed(320.0)),
        choose_btn,
    ]
    .spacing(ZONE_GAP * 2)
    .align_y(Alignment::Center);

    let can_confirm = install_root.is_some()
        && available.is_some()
        && state.install_in_progress.is_none();
    let mut confirm_btn = button(text(if state.install_in_progress == Some(channel) {
        "Installing…"
    } else {
        "Confirm"
    }))
    .padding(8);
    if can_confirm {
        confirm_btn = confirm_btn.on_press(Message::InstallConfirmed);
    }

    let buttons = row![
        button(text("Cancel")).on_press(Message::CloseCenterMenu).padding(8),
        confirm_btn,
    ]
    .spacing(ZONE_GAP * 2);

    let status_line: Element<'_, Message> = if let Some(err) = error {
        text(err.to_string()).size(13).into()
    } else if state.install_in_progress == Some(channel) {
        text(format!(
            "Downloading + extracting to {}/{}…",
            install_root
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<pending>".into()),
            channel.dir_name()
        ))
        .size(13)
        .into()
    } else if is_update {
        let existing = install_root
            .map(|p| p.join(channel.dir_name()).to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("<existing folder>/{}", channel.dir_name()));
        text(format!("Updating existing install at: {existing} — location locked"))
            .size(13)
            .into()
    } else {
        text(format!(
            "Files will be installed into: <chosen folder>/{}/",
            channel.dir_name()
        ))
        .size(13)
        .into()
    };

    column![
        text(format!("Channel: {}", channel.label())).size(16),
        version_cell,
        path_row,
        buttons,
        status_line,
    ]
    .spacing(ZONE_GAP * 3)
    .align_x(Alignment::Center)
    .into()
}

/// Convenience for `center/mod.rs`'s dispatch arm.
pub fn dispatch<'a>(state: &'a AppState, view_state: &'a CenterView) -> Element<'a, Message> {
    if let CenterView::InstallPrompt {
        channel,
        install_root,
        available,
        error,
    } = view_state
    {
        view(
            state,
            *channel,
            install_root.as_ref(),
            available.as_deref(),
            error.as_deref(),
        )
    } else {
        // Shouldn't be reachable — center/mod.rs only dispatches here for
        // the matching variant. Fall back to a blank pane defensively.
        container(text("")).into()
    }
}
