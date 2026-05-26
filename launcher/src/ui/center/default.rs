//! Default center view: title + "no menu selected" placeholder.
//! Foundation doc §5C: v1 center state.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, TITLE_SIZE};
use iced::widget::{column, container, text};
use iced::{Alignment, Element, Length};

pub fn view(_state: &AppState) -> Element<'_, Message> {
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
