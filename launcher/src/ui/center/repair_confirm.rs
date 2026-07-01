//! Repair confirmation prompt. Triggered by Settings → Game Channel Management
//! → "Repair" on an installed row. Repair re-downloads and reinstalls the
//! *installed* version (fetch-by-tag) through the transactional install
//! pipeline — so a failed repair leaves the prior install intact — and auto
//! re-verifies on success. Progress is shown in the shared bottom bar.

use crate::app::{AppState, CenterView, Message};
use crate::channel::Channel;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view<'a>(
    state: &'a AppState,
    channel: Channel,
    version: &'a str,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let header = row![
        text(format!("Repair {} Game", channel.label())).size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
    ]
    .align_y(Alignment::Center);

    container(
        column![header, super::scroll_area(content(state, channel, version, error))]
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
    version: &'a str,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let summary = container(text(format!(
        "Re-download and reinstall {} v{}.",
        channel.label(),
        version
    )))
    .style(theme::bordered)
    .padding(12)
    .width(Length::Fixed(460.0))
    .center_x(Length::Fixed(460.0));

    let explainer = text(
        "Repair fixes a corrupted or incomplete install by reinstalling the \
         same version. Your saves are not affected. If the download fails, your \
         current install is left untouched.",
    )
    .size(13);

    let in_progress = state.install_in_progress == Some(channel);
    // Disabled while any install / uninstall is running or the game is open —
    // mirrors the Settings-row gating and blocks a double-press of this Confirm.
    let can_repair = state.install_in_progress.is_none()
        && state.uninstall_in_progress.is_none()
        && !state.game_running;
    let mut repair_btn = button(text(if in_progress {
        "Repairing\u{2026}"
    } else {
        "Repair"
    }))
    .padding(8);
    if can_repair {
        repair_btn = repair_btn.on_press(Message::RepairConfirmed);
    }
    let buttons = row![
        button(text("Cancel"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
        repair_btn,
    ]
    .spacing(ZONE_GAP * 2);

    let status_line: Element<'_, Message> = if let Some(err) = error {
        text(err.to_string()).size(13).into()
    } else if in_progress {
        text("Downloading and reinstalling\u{2026} progress is shown in the bottom bar.")
            .size(12)
            .into()
    } else {
        text("").size(12).into()
    };

    column![summary, explainer, buttons, status_line]
        .spacing(ZONE_GAP * 3)
        .align_x(Alignment::Center)
        .into()
}

/// Dispatch convenience matching the pattern used by uninstall_confirm.rs.
pub fn dispatch<'a>(state: &'a AppState, view_state: &'a CenterView) -> Element<'a, Message> {
    if let CenterView::RepairConfirm {
        channel,
        version,
        error,
    } = view_state
    {
        view(state, *channel, version, error.as_deref())
    } else {
        container(text("")).into()
    }
}
