//! Default center view: the focused channel's game changelog.
//!
//! Follows `state.selected_channel`, anchored at the version that channel has
//! **installed** — that entry on top, the nine before it, nothing newer. So the
//! pane reads as "what you have" rather than "what exists"; anything newer is
//! what the update prompt is for.
//!
//! A channel with nothing installed has no anchor, so it keeps the original
//! "no menu selected" placeholder (Foundation doc §5C).

use crate::app::handlers::changelog as handler;
use crate::app::{AppState, Message};
use crate::changelog::Kind;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{column, container, text};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    let channel = state.selected_channel;
    let installed = state
        .identity
        .channels
        .get(&channel)
        .and_then(|c| c.parsed_installed_version());

    let Some(installed) = installed else {
        return placeholder();
    };

    let entries = crate::changelog::anchored(
        state.changelog.entries(Kind::Game),
        handler::shipped_for(state, channel),
        &installed,
        handler::WINDOW,
    );
    let open = handler::open_set(state, Kind::Game, &entries);

    let header = column![
        text(format!("{} Changelog", channel.label())).size(TITLE_SIZE),
        text(format!("Installed: v{installed}")).size(14),
    ]
    .spacing(ZONE_GAP)
    .align_x(Alignment::Center);

    let body = super::changelog::view(
        Kind::Game,
        &entries,
        &open,
        "No changelog entries for this channel yet.",
    );

    container(
        column![header, super::scroll_area(body)]
            .spacing(ZONE_GAP * 4)
            .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}

/// The pre-changelog placeholder, kept for a channel with no install.
fn placeholder<'a>() -> Element<'a, Message> {
    container(
        column![
            text("Briska Blast").size(TITLE_SIZE),
            text(""),
            text("No menu selected.").size(14),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding(16)
    .into()
}
