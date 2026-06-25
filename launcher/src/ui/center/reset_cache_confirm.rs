//! Reset Runtime Cache confirmation (Windows). Deletes the game's .NET
//! runtime-extraction cache (`%LOCALAPPDATA%\data_BriskaBlast…_windows_x86_64`)
//! so the game rebuilds it on next launch — for the case where Verify File
//! Integrity passes but the game still won't start. Never touches antivirus;
//! the game install and saves are untouched.

use crate::app::{AppState, CenterView, Message};
use crate::channel::Channel;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view<'a>(channel: Channel, error: Option<&'a str>) -> Element<'a, Message> {
    let header = row![
        text(format!("Reset {} Runtime Cache", channel.label())).size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
    ]
    .align_y(Alignment::Center);

    let explainer = text(
        "This deletes the game's runtime cache so it rebuilds cleanly the next \
         time you launch. Your game install and saves are not affected. Use this \
         if the game won't start even though Verify File Integrity passed.",
    )
    .size(14);

    let note = text(
        "If the game still won't start after this, your antivirus may be \
         removing its files — add an exclusion for the game folder yourself.",
    )
    .size(12);

    let buttons = row![
        button(text("Cancel"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
        button(text("Reset Runtime Cache"))
            .on_press(Message::ResetRuntimeCacheConfirmed)
            .padding(8),
    ]
    .spacing(ZONE_GAP * 2);

    let status: Element<'_, Message> = match error {
        Some(err) => text(err.to_string()).size(13).into(),
        None => text("").size(12).into(),
    };

    container(
        column![header, explainer, note, buttons, status]
            .spacing(ZONE_GAP * 3)
            .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}

/// Dispatch convenience matching the other center-view prompts.
pub fn dispatch<'a>(_state: &'a AppState, view_state: &'a CenterView) -> Element<'a, Message> {
    if let CenterView::ResetCacheConfirm { channel, error } = view_state {
        view(*channel, error.as_deref())
    } else {
        container(text("")).into()
    }
}
