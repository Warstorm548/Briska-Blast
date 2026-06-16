//! Zone 2: channel picker + per-channel "Check for Updates" on top, update
//! verdict box in the middle, server-status dots below.
//! Dev row is filtered out for unflagged users via `state.visible_channels`.

use crate::app::{AppState, ChannelUpdateStatus, Message};
use crate::ui::theme::{self, RAIL_WIDTH, ZONE_GAP};
use iced::widget::{button, column, container, pick_list, text};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    column![channel_box(state), channel_update_box(state), server_status_box(state),]
        .spacing(ZONE_GAP)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .into()
}

fn channel_box(state: &AppState) -> Element<'_, Message> {
    // While the game is running we render the channel as a static label
    // instead of the interactive picker — switching channels mid-session
    // would race the spawned process against the channel-scoped install
    // dir (foundation §5E). Restored to a pick_list as soon as the game
    // exits.
    let picker: Element<'_, Message> = if state.game_running {
        container(text(state.selected_channel.label()).size(16))
            .style(theme::bordered)
            .padding(8)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .into()
    } else {
        pick_list(
            state.visible_channels.clone(),
            Some(state.selected_channel),
            Message::ChannelPicked,
        )
        .width(Length::Fill)
        .into()
    };
    // Manual "Check for Updates" for the focused channel — re-queries GitHub on
    // demand so a release published since boot is picked up. A found update
    // flips the bottom-bar Update button (it reads the same available_versions
    // this check refreshes); the verdict lands in channel_update_box below.
    // Gated on an installed channel, same as the old Settings row: the
    // up-to-date / update verdict is only meaningful against a version on disk.
    let channel = state.selected_channel;
    let installed = state
        .identity
        .channels
        .get(&channel)
        .map(|creds| creds.install_location.is_some() && creds.installed_version.is_some())
        .unwrap_or(false);
    let busy = state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running;
    let checking = matches!(
        state.channel_update_status.get(&channel),
        Some(ChannelUpdateStatus::Checking)
    );
    let check_label = if checking { "Checking\u{2026}" } else { "Check for Updates" };
    let mut check_btn = button(text(check_label)).width(Length::Fill).padding(8);
    if installed && !busy && !checking {
        check_btn = check_btn.on_press(Message::CheckChannelUpdatePressed(channel));
    }
    container(
        column![text("Channel").size(14), picker, check_btn].spacing(ZONE_GAP * 2),
    )
    .style(theme::bordered)
    .padding(8)
    .width(Length::Fill)
    .into()
}

/// Verdict box for the manual per-channel update check, shown directly under
/// the channel picker. Reads `channel_update_status` for the focused channel
/// only; `nav::channel_picked` drops completed verdicts on a channel switch
/// (keeping in-flight checks), so the box reflects the channel currently on
/// screen — em-dash until checked, or "Checking…" while a fetch is in flight.
fn channel_update_box(state: &AppState) -> Element<'_, Message> {
    let channel = state.selected_channel;
    let verdict = match state.channel_update_status.get(&channel) {
        None => "\u{2014}".to_string(), // em dash — not checked for this channel
        Some(ChannelUpdateStatus::Checking) => "Checking\u{2026}".to_string(),
        Some(ChannelUpdateStatus::UpdateAvailable(v)) => {
            format!("\u{25CF} Update available \u{2014} v{v}")
        }
        Some(ChannelUpdateStatus::UpToDate(v)) => {
            format!("\u{2713} You're up to date \u{2014} v{v}")
        }
        Some(ChannelUpdateStatus::Failed) => "\u{26A0} Check failed".to_string(),
        Some(ChannelUpdateStatus::RateLimited { resume_at }) => {
            format!("\u{23F3} GitHub limit \u{2014} retry at {resume_at}")
        }
    };
    container(
        column![
            text(format!("Updates \u{00b7} {}", channel.label())).size(13),
            text(verdict).size(13),
        ]
        .spacing(ZONE_GAP),
    )
    .style(theme::bordered)
    .padding(8)
    .width(Length::Fill)
    .into()
}

fn server_status_box(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Server status").size(14)].spacing(ZONE_GAP);
    for channel in &state.visible_channels {
        let reachable = state
            .server_reachable
            .get(channel)
            .copied()
            .unwrap_or(false);
        let dot = if reachable { "\u{25CF}" } else { "\u{25CB}" };
        col = col.push(text(format!("{}  {}", dot, channel.label())));
    }
    container(col)
        .style(theme::bordered)
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
