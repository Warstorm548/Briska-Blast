//! Zone 1: launcher version | branch-updates banner + launcher-update banner | gear button.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, BAR_HEIGHT, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    row![
        version_label(),
        branch_banner(state),
        launcher_banner(state),
        gear_button(),
    ]
    .spacing(ZONE_GAP)
    .align_y(Alignment::Center)
    .into()
}

fn version_label() -> Element<'static, Message> {
    container(text(format!("v{}", env!("CARGO_PKG_VERSION"))))
        .style(theme::bordered)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .center_y(Length::Fill)
        .padding(8)
        .into()
}

fn branch_banner(state: &AppState) -> Element<'_, Message> {
    let branch_text = if state.branch_updates_available.is_empty() {
        "All branches up to date".to_string()
    } else {
        let names: Vec<&str> = state
            .branch_updates_available
            .iter()
            .map(|c| c.label())
            .collect();
        format!("Updates available: {}", names.join(", "))
    };
    container(text(branch_text))
        .style(theme::bordered)
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .center_y(Length::Fill)
        .padding(8)
        .into()
}

fn launcher_banner(state: &AppState) -> Element<'_, Message> {
    let launcher_text = if state.launcher_update_available {
        "Update available: launcher"
    } else {
        "Launcher up to date"
    };
    let inner: Element<'_, Message> = if state.launcher_update_available {
        button(text(launcher_text))
            .on_press(Message::LauncherUpdatePressed)
            .width(Length::Fill)
            .into()
    } else {
        text(launcher_text).into()
    };
    container(inner)
        .style(theme::bordered)
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .center_y(Length::Fill)
        .padding(8)
        .into()
}

fn gear_button() -> Element<'static, Message> {
    container(
        button(text("\u{2699}").size(22))
            .on_press(Message::OpenSettings)
            .padding(8),
    )
    .style(theme::bordered)
    .width(Length::Fixed(BAR_HEIGHT as f32))
    .height(Length::Fixed(BAR_HEIGHT as f32))
    .padding(4)
    .into()
}
