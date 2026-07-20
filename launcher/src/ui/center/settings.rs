//! Settings center view: header + tab bar + active-tab body.
//! Matches mockup Example Imgs/LuncherSettings.png.

use crate::app::{AppState, Message, SettingsTab};
use crate::channel::Channel;
use crate::firewall::FirewallStatus;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use crate::updater::branches::VerifyOutcome;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

pub fn view(state: &AppState, active: SettingsTab) -> Element<'_, Message> {
    container(
        column![header_row(), tab_bar(active), super::scroll_area(body(state, active))]
            .spacing(ZONE_GAP * 3),
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
        SettingsTab::ChannelManagement => {
            // `mut` is only exercised by the Windows-gated push below.
            #[allow(unused_mut)]
            let mut col = column![
                channels_section(state),
                important_files_section(state),
                firewall_section(state),
            ]
            .spacing(ZONE_GAP * 4);
            // Reset Runtime Cache is Windows-only this ship — the cache only
            // lives outside the install dir on Windows (Linux/macOS keep it
            // where Verify/Repair already cover it).
            #[cfg(target_os = "windows")]
            {
                col = col.push(runtime_cache_section(state));
            }
            col.into()
        }
        // No Length::Fill here: this body is placed inside a `scroll_area`, where a
        // Fill height on the scroll axis is undefined. A padded, horizontally
        // centered placeholder is enough for a "coming soon" tab.
        SettingsTab::Graphics => container(text("Coming soon.").size(16))
            .center_x(Length::Fill)
            .width(Length::Fill)
            .padding(24)
            .into(),
        SettingsTab::LauncherOptions => super::launcher_update::content(state),
    }
}

fn channels_section(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Channels").size(20)].spacing(ZONE_GAP);
    // Buttons are disabled when no install is on record for the channel,
    // or when an install / play action is already in flight. Stage 7
    // change — Stage 3-6's settings tab unconditionally enabled them.
    // An in-flight verify (global single-flight, possibly multi-second) counts
    // as busy too, so Verify/Repair/Uninstall are all disabled while one runs.
    let busy = state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
        || state.verify_in_progress.is_some();
    for c in &state.visible_channels {
        let c = *c;
        let installed = state
            .identity
            .channels
            .get(&c)
            .map(|creds| creds.install_location.is_some() && creds.installed_version.is_some())
            .unwrap_or(false);
        let actionable = installed && !busy;
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button_maybe("Uninstall", Message::UninstallChannel(c), actionable),
                cell_button_maybe(
                    "Verify File Integrity",
                    Message::VerifyChannel(c),
                    actionable,
                ),
                cell_button_maybe("Repair", Message::RepairChannel(c), actionable),
                verify_status_cell(state, c),
            ]
            .spacing(ZONE_GAP),
        );
    }
    col.into()
}

fn important_files_section(state: &AppState) -> Element<'_, Message> {
    let mut col = column![text("Game Important Files").size(20)].spacing(ZONE_GAP);
    let busy = state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running;
    for c in &state.visible_channels {
        let c = *c;
        let installed = state
            .identity
            .channels
            .get(&c)
            .map(|creds| creds.install_location.is_some() && creds.installed_version.is_some())
            .unwrap_or(false);
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button_maybe("Game Save", Message::GameSavePressed(c), installed && !busy),
                cell_button_maybe("Logs", Message::GameLogsPressed(c), installed && !busy),
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

fn verify_status_cell(state: &AppState, channel: Channel) -> Element<'_, Message> {
    let label = if state.verify_in_progress == Some(channel) {
        "\u{2026} Verifying\u{2026}".to_string()
    } else {
        match state.verify_results.get(&channel) {
            None => "\u{2014}".to_string(), // em dash — not yet verified this launch
            Some(VerifyOutcome::Ok { version }) => format!("\u{2713} Verified v{version}"),
            Some(VerifyOutcome::ManifestMissing) => "\u{2717} Manifest missing".to_string(),
            Some(VerifyOutcome::ManifestUnreadable(_)) => "\u{2717} Manifest unreadable".to_string(),
            Some(VerifyOutcome::ExecutableMissing { .. }) => {
                "\u{2717} Executable missing".to_string()
            }
            Some(VerifyOutcome::FilesMissing { count, .. }) => {
                format!("\u{2717} {count} file(s) missing")
            }
            Some(VerifyOutcome::FilesCorrupted { count, .. }) => {
                format!("\u{2717} {count} file(s) corrupted")
            }
        }
    };
    container(text(label).size(13))
        .style(theme::bordered)
        .padding(8)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fixed(36.0))
        .into()
}

/// Windows-Firewall inbound-rule status per channel (P3). The check is
/// non-elevated (read-only `netsh show`) and button-triggered, mirroring the
/// Verify File Integrity row. On Linux the resolved status is `NotApplicable`,
/// which the status cell renders as an explanatory note rather than hiding the
/// row — outbound hole-punching traverses the default firewall there, so there
/// is genuinely nothing to add.
fn firewall_section(state: &AppState) -> Element<'_, Message> {
    let mut col = column![
        text("Firewall (game hosting)").size(20),
        text(
            "Hosting a match needs an inbound firewall rule for the game. \
             Checking is read-only and needs no admin rights."
        )
        .size(13),
    ]
    .spacing(ZONE_GAP);
    let busy = state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running;
    for c in &state.visible_channels {
        let c = *c;
        let installed = state
            .identity
            .channels
            .get(&c)
            .map(|creds| creds.install_location.is_some() && creds.installed_version.is_some())
            .unwrap_or(false);
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button_maybe("Check Firewall", Message::CheckFirewall(c), installed && !busy),
                firewall_status_cell(state, c),
            ]
            .spacing(ZONE_GAP),
        );
    }
    col.into()
}

fn firewall_status_cell(state: &AppState, channel: Channel) -> Element<'_, Message> {
    let label = match state.firewall_status.get(&channel) {
        None => "\u{2014}".to_string(), // em dash — not checked this launch
        Some(FirewallStatus::RulePresent) => "\u{2713} Rule present".to_string(),
        Some(FirewallStatus::NotDetected) => "\u{2717} No inbound rule".to_string(),
        Some(FirewallStatus::Unknown(_)) => "? Check failed".to_string(),
        Some(FirewallStatus::NotApplicable) => "N/A — not applicable on this OS".to_string(),
    };
    container(text(label).size(13))
        .style(theme::bordered)
        .padding(8)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fixed(36.0))
        .into()
}

/// Reset Runtime Cache rows (Windows only). Mirrors the firewall section's
/// shape: a short explainer plus a per-channel button. Deleting the cache lets
/// the game rebuild its .NET runtime on next launch — the fix for "Verify
/// passed but the game still won't start". Never touches antivirus.
#[cfg(target_os = "windows")]
fn runtime_cache_section(state: &AppState) -> Element<'_, Message> {
    let mut col = column![
        text("Game Runtime").size(20),
        text(
            "If the game won't start even though Verify File Integrity passed, \
             reset its runtime cache — the game rebuilds it cleanly on the next \
             launch. If the problem keeps coming back, your antivirus may be \
             removing the game's files; add an exclusion for the game folder."
        )
        .size(13),
    ]
    .spacing(ZONE_GAP);
    let busy = state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running;
    for c in &state.visible_channels {
        let c = *c;
        let installed = state
            .identity
            .channels
            .get(&c)
            .map(|creds| creds.install_location.is_some() && creds.installed_version.is_some())
            .unwrap_or(false);
        col = col.push(
            row![
                bordered_cell(c.label(), 120.0),
                cell_button_maybe(
                    "Reset Runtime Cache",
                    Message::ResetRuntimeCache(c),
                    installed && !busy,
                ),
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

#[allow(dead_code)] // kept for parity with other Settings rows; may be re-used
fn cell_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    button(text(label))
        .on_press(msg)
        .width(Length::Fill)
        .padding(8)
        .into()
}

/// Same as `cell_button` but drops `.on_press` when `enabled` is false so
/// Iced renders the button non-interactive (foundation §5E pattern reused
/// from the bottom-bar Update / Play cells).
fn cell_button_maybe(
    label: &'static str,
    msg: Message,
    enabled: bool,
) -> Element<'static, Message> {
    let mut btn = button(text(label)).width(Length::Fill).padding(8);
    if enabled {
        btn = btn.on_press(msg);
    }
    btn.into()
}
