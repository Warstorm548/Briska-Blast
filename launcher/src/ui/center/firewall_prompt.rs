//! First-Play firewall prompt (Windows, option A).
//!
//! Shown when the user hits Play for a channel that has no inbound firewall
//! rule yet (resolved by the non-elevated check in `app.rs`). Hosting a match
//! needs that rule; creating it requires a single UAC elevation. The user can
//! Allow (run the elevated `netsh add rule`, then launch) or Skip (launch now
//! without a rule — hosting may not work). The prompt stays open with `error`
//! populated if the elevated add fails, so the user can retry or skip.

use crate::app::{AppState, CenterView, Message};
use crate::channel::Channel;
use crate::firewall;
use crate::ui::theme::{self, TITLE_SIZE, ZONE_GAP};
use iced::widget::{button, column, container, text, Space};
use iced::{Alignment, Element, Length};
use std::path::Path;

pub fn view<'a>(
    state: &'a AppState,
    channel: Channel,
    exe: &'a Path,
    in_progress: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    // While the elevation is in flight the Close button is the only escape; the
    // game isn't running yet so there's nothing to interrupt.
    let header = row_header(channel);

    container(
        column![
            header,
            super::scroll_area(content(state, channel, exe, in_progress, error))
        ]
        .spacing(ZONE_GAP * 4)
        .align_x(Alignment::Center),
    )
    .style(theme::menu_pane)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(16)
    .into()
}

fn row_header<'a>(channel: Channel) -> Element<'a, Message> {
    use iced::widget::row;
    row![
        text(format!("Allow {} Game to host?", channel.label())).size(TITLE_SIZE),
        Space::new().width(Length::Fill),
        button(text("Close"))
            .on_press(Message::CloseCenterMenu)
            .padding(8),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn content<'a>(
    _state: &'a AppState,
    channel: Channel,
    exe: &'a Path,
    in_progress: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    use iced::widget::row;

    let explain = text(
        "Hosting a match needs an inbound firewall rule for the game. Allowing \
         it shows a Windows permission prompt (admin). You can play without it, \
         but hosting may not work until the rule is added.",
    )
    .size(14);

    let rule_row = container(
        text(format!("Rule: {}", firewall::rule_name(channel))).size(13),
    )
    .style(theme::bordered)
    .padding(10)
    .width(Length::Fixed(460.0));

    let exe_row = container(text(exe.display().to_string()).size(12))
        .style(theme::bordered)
        .padding(10)
        .width(Length::Fixed(460.0));

    // Both buttons are inert while the elevated task runs.
    let allow_label = if in_progress {
        "Requesting permission\u{2026}"
    } else {
        "Allow & Play"
    };
    let mut allow_btn = button(text(allow_label)).padding(8);
    let mut skip_btn = button(text("Skip & Play")).padding(8);
    if !in_progress {
        allow_btn = allow_btn.on_press(Message::FirewallPromptAllow);
        skip_btn = skip_btn.on_press(Message::FirewallPromptSkip);
    }
    let buttons = row![skip_btn, allow_btn].spacing(ZONE_GAP * 2);

    let status_line: Element<'_, Message> = if let Some(err) = error {
        text(err.to_string()).size(13).into()
    } else {
        Space::new().height(Length::Fixed(0.0)).into()
    };

    column![explain, rule_row, exe_row, buttons, status_line]
        .spacing(ZONE_GAP * 3)
        .align_x(Alignment::Center)
        .into()
}

/// Dispatch convenience matching the pattern used by uninstall_confirm.rs.
pub fn dispatch<'a>(state: &'a AppState, view_state: &'a CenterView) -> Element<'a, Message> {
    if let CenterView::FirewallPrompt {
        channel,
        exe,
        in_progress,
        error,
        ..
    } = view_state
    {
        view(state, *channel, exe, *in_progress, error.as_deref())
    } else {
        container(text("")).into()
    }
}
