//! Zone 5: Update button | progress bar | Play button.
//! Buttons drop their .on_press when game is running (Iced renders them
//! non-pressable without a separate "disabled" state).

use crate::app::{AppState, Message};
use crate::ui::theme::{self, BAR_HEIGHT, RAIL_WIDTH, ZONE_GAP};
use crate::updater::branches::InstallProgress;
use iced::widget::{button, column, container, progress_bar, row, text};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState) -> Element<'_, Message> {
    row![
        update_cell(state),
        progress_cell(state),
        play_cell(state),
    ]
    .spacing(ZONE_GAP)
    .into()
}

fn update_cell(state: &AppState) -> Element<'_, Message> {
    // Stage 4 button state machine. Drives label + enabled from three
    // inputs: installed_version (parsed semver, see ChannelCreds::
    // parsed_installed_version), available_versions[channel] (cached
    // from GitHub at boot / dev_flag flip), and game_running /
    // install_in_progress.
    //
    // Full table:
    //   game running OR install in flight → "Running"/"Installing…" disabled
    //   not installed  + available Some(v) → "Install <C> Game"          enabled
    //   not installed  + available None    → "Install <C> Game"          disabled
    //   installed v_i  + available v_a > i → "Update to vX.Y.Z"          enabled
    //   installed v_i  + available v_a ≤ i → "Up to date — vX.Y.Z"       disabled
    //   installed v_i  + available None    → "Up to date — vX.Y.Z"       disabled
    let channel = state.selected_channel;
    let creds = state.identity.channels.get(&channel);

    // Half-state guard from Stage 3 review — only count installed when both
    // install_location AND installed_version are present. The parsed helper
    // returns None on any half-state or unparseable version string.
    if let Some(c) = creds {
        if c.install_location.is_some() != c.installed_version.is_some() {
            tracing::warn!(
                channel = %channel,
                has_location = c.install_location.is_some(),
                has_version = c.installed_version.is_some(),
                "channel install state inconsistent — treating as not installed"
            );
        }
    }
    let installed = creds.and_then(|c| c.parsed_installed_version());
    let available = state.available_versions.get(&channel);

    let (label, enabled): (String, bool) = if state.game_running {
        ("Running".to_string(), false)
    } else if state.install_in_progress.is_some() {
        // Any in-flight install disables the button — not just one
        // targeting the selected channel. Otherwise switching the
        // dropdown mid-install would re-enable a button whose press the
        // UpdatePressed handler already refuses (install_in_progress
        // guard).
        ("Installing\u{2026}".to_string(), false)
    } else {
        match (installed.as_ref(), available) {
            // Update available: newer remote version than what's on disk.
            (Some(_inst), Some(avail)) if avail > installed.as_ref().unwrap() => {
                (format!("Update to v{avail}"), true)
            }
            // Installed, no newer release (either equal or remote unknown).
            (Some(inst), _) => (format!("Up to date \u{2014} v{inst}"), false),
            // Not installed, release is known — user can install.
            (None, Some(_)) => (format!("Install {} Game", channel.label()), true),
            // Not installed and remote is unknown (fetch in flight or
            // failed) — show the install label but leave it disabled so the
            // user has nothing to click against unresolved state.
            (None, None) => (format!("Install {} Game", channel.label()), false),
        }
    };

    let mut btn = button(text(label))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT as f32));
    if enabled {
        btn = btn.on_press(Message::UpdatePressed);
    }
    container(btn)
        .style(theme::bordered)
        .width(Length::Fixed(RAIL_WIDTH as f32))
        .height(Length::Fixed(BAR_HEIGHT as f32))
        .into()
}

fn progress_cell(state: &AppState) -> Element<'_, Message> {
    // Stage 6: driven from state.download_progress (last InstallProgress
    // event from the active install pipeline). Renders an idle placeholder
    // when no install is in flight.
    let (fraction, label): (f32, String) = match &state.download_progress {
        Some(InstallProgress::Downloading {
            fraction,
            bytes_now,
            bytes_total,
        }) => {
            // When the server omits Content-Length, bytes_total is 0 and
            // fraction is meaningless. Render an indeterminate bar (0)
            // and drop the percent / total from the label rather than
            // showing "0% (… / 0 B)".
            if *bytes_total > 0 {
                (
                    (*fraction).clamp(0.0, 1.0),
                    format!(
                        "Downloading \u{2014} {:.0}% ({} / {})",
                        fraction * 100.0,
                        format_bytes(*bytes_now),
                        format_bytes(*bytes_total),
                    ),
                )
            } else {
                (
                    0.0,
                    format!(
                        "Downloading \u{2014} {} (unknown total)",
                        format_bytes(*bytes_now)
                    ),
                )
            }
        }
        Some(InstallProgress::Extracting) => (1.0, "Extracting\u{2026}".to_string()),
        Some(InstallProgress::Done) => (1.0, "Done.".to_string()),
        None => (0.0, "Idle".to_string()),
    };
    container(
        column![
            progress_bar(0.0..=1.0, fraction),
            text(label).size(12),
        ]
        .spacing(4)
        .align_x(Alignment::Center),
    )
    .style(theme::bordered)
    .width(Length::Fill)
    .height(Length::Fixed(BAR_HEIGHT as f32))
    .center_y(Length::Fill)
    .padding(8)
    .into()
}

fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if n >= GIB {
        format!("{:.2} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KiB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
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
