//! Settings center view: header + tab bar + active-tab body.
//! Matches mockup Example Imgs/LuncherSettings.png.

use crate::app::{AppState, Message, SettingsTab};
use crate::channel::Channel;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState, active: SettingsTab) -> Element<'_, Message> {
    container(
        column![header_row(), tab_bar(active), body(state, active)].spacing(ZONE_GAP * 3),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}

fn header_row() -> Element<'static, Message> {
    row![
        text("Settings").size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close")).on_press(Message::CloseCenterMenu).padding(8),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn tab_bar(active: SettingsTab) -> Element<'static, Message> {
    row![
        tab_button("Game Channel Management", SettingsTab::ChannelManagement, active),
        tab_button("Game Graphics Settings", SettingsTab::Graphics, active),
        tab_button("Launcher Options", SettingsTab::LauncherOptions, active),
    ]
    .spacing(ZONE_GAP)
    .into()
}

fn tab_button(
    label: &'static str,
    tab: SettingsTab,
    active: SettingsTab,
) -> Element<'static, Message> {
    let is_active = tab == active;
    button(text(label))
        .style(move |theme, status| {
            if is_active {
                theme::tab_active(theme, status)
            } else {
                theme::tab_inactive(theme, status)
            }
        })
        .on_press(Message::SettingsTabSelected(tab))
        .padding(8)
        .into()
}

fn body<'a>(state: &'a AppState, active: SettingsTab) -> Element<'a, Message> {
    match active {
        SettingsTab::ChannelManagement => column![channels_section(), important_files_section()]
            .spacing(ZONE_GAP * 4)
            .into(),
        SettingsTab::Graphics => container(text("Coming soon.").size(16))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        SettingsTab::LauncherOptions => super::launcher_update::content(state),
    }
}

fn channels_section() -> Element<'static, Message> {
    let mut col = column![text("Channels").size(20)].spacing(ZONE_GAP);
    for c in Channel::all() {
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button("Uninstall", Message::UninstallChannel(c)),
                cell_button("Verify File Integrity", Message::VerifyChannel(c)),
            ]
            .spacing(ZONE_GAP),
        );
    }
    col.into()
}

fn important_files_section() -> Element<'static, Message> {
    let mut col = column![text("Game Important Files").size(20)].spacing(ZONE_GAP);
    for c in Channel::all() {
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button("Game Save", Message::GameSavePressed(c)),
                container(Space::new().width(Length::Fill))
                    .style(theme::bordered)
                    .width(Length::Fill)
                    .height(Length::Fixed(36.0)),
            ]
            .spacing(ZONE_GAP),
        );
    }
    col.into()
}

fn bordered_cell(label: &'static str, width: f32) -> Element<'static, Message> {
    container(text(label))
        .style(theme::bordered)
        .padding(8)
        .center_x(Length::Fixed(width))
        .into()
}

fn cell_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    button(text(label))
        .on_press(msg)
        .width(Length::Fill)
        .padding(8)
        .into()
}
