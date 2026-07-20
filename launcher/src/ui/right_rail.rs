//! Zone 4: username + Change Name button + per-channel Player IDs.
//! Dev row hidden when not in `state.visible_channels`.

use crate::app::{AppState, Message};
use crate::ui::theme::{self, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, column, container, text};
use iced::{Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    column![username_box(state), player_ids_box(state),]
        .spacing(ZONE_GAP)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .into()
}

fn username_box(state: &AppState) -> Element<'_, Message> {
    // Drop the on_press while a game is running so the button renders
    // non-pressable (Iced's idiom for disabled — same pattern as Play/Update in
    // the bottom bar). The username must not change mid-session; the lock holds
    // even across a launcher restart because `game_running` is recovered by
    // probing the game's liveness socket (`game_instance.json`) on boot. The
    // handler re-checks `game_running` too.
    let mut change_btn = button(text("Change Name"));
    if !state.game_running {
        change_btn = change_btn.on_press(Message::ChangeNamePressed);
    }

    let mut col = column![text(state.identity.username.clone()).size(18), change_btn]
        .spacing(ZONE_GAP * 2);

    if state.game_running {
        col = col.push(text("Locked while in game").size(11));
    }
    // Transient notice — e.g. the server rejected a name and we reverted.
    if let Some(notice) = &state.username_notice {
        col = col.push(text(notice.clone()).size(11));
    }

    container(col)
        .style(theme::bordered)
        .padding(8)
        .width(Length::Fill)
        .into()
}

fn player_ids_box(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Player IDs").size(14)].spacing(ZONE_GAP);
    for channel in &state.visible_channels {
        if let Some(creds) = state.identity.channels.get(channel) {
            col = col.push(text(format!(
                "{} #{}",
                channel.label(),
                creds.player_id
            )));
        }
    }
    container(col)
        .style(theme::bordered)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
