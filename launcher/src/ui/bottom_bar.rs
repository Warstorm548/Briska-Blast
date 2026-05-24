//! Zone 5: Update button | progress placeholder | Play button.
//! Buttons drop their .on_press when game is running (Iced renders them
//! non-pressable without a separate "disabled" state).

use crate::app::{AppState, Message};
use crate::mock::MOCK_PROGRESS_PERCENT;
use crate::ui::theme::{self, BAR_HEIGHT, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, container, row, text};
use iced::{Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    row![
        update_cell(state),
        progress_cell(),
        play_cell(state),
    ]
    .spacing(ZONE_GAP)
    .into()
}

fn update_cell(state: &AppState) -> Element<'_, Message> {
    // Stage 3: when the selected channel has no install yet, the button
    // becomes "Install <Channel> Game" and routes to the install prompt.
    // The installed-but-outdated / installed-and-up-to-date label states
    // land in Stage 4 (version detection + state machine).
    let installed = state
        .identity
        .channels
        .get(&state.selected_channel)
        .and_then(|c| c.install_location.as_ref())
        .is_some();
    let label: String = if state.install_in_progress == Some(state.selected_channel) {
        "Installing\u{2026}".into()
    } else if installed {
        "Update".into()
    } else {
        format!("Install {} Game", state.selected_channel.label())
    };
    let mut btn = button(text(label))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32));
    if !state.game_running && state.install_in_progress.is_none() {
        btn = btn.on_press(Message::UpdatePressed);
    }
    container(btn)
        .style(theme::bordered)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .into()
}

fn progress_cell() -> Element<'static, Message> {
    container(text(format!(
        "Progress placeholder \u{2014} {}% done",
        MOCK_PROGRESS_PERCENT
    )))
    .style(theme::bordered)
    .width(Length::Fill)
    .height(Length::Fixed(BAR_HEIGHT as f32))
    .center_y(Length::Fill)
    .padding(8)
    .into()
}

fn play_cell(state: &AppState) -> Element<'_, Message> {
    let label = if state.game_running { "Running" } else { "Play" };
    let mut btn = button(text(label))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32));
    if !state.game_running {
        btn = btn.on_press(Message::PlayPressed);
    }
    container(btn)
        .style(theme::bordered)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .into()
}
