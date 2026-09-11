//! Shared sizing constants + container style helpers so the five zones
//! agree on dimensions and box boundaries.

use iced::widget::{button, container, progress_bar};
use iced::{Background, Border, Color, Theme};

pub const ZONE_GAP: u32 = 4;
pub const RAIL_WIDTH: u32 = 220;
pub const BAR_HEIGHT: u32 = 48;
pub const TITLE_SIZE: u32 = 32;

/// Height of the update progress bar. Tall enough to seat the status text
/// inside it with room to breathe, and still to fit within [`BAR_HEIGHT`] once
/// the surrounding container's padding is taken off.
pub const PROGRESS_HEIGHT: u32 = 26;

/// Text drawn on the progress bar. Black, per the launcher's update-progress
/// design — which is why [`progress_track`] below makes both halves of the bar
/// light. Fixed rather than theme-derived: it has to stay legible against the
/// bar's own colours, not against the window behind it.
pub const PROGRESS_TEXT: Color = Color::BLACK;

/// The update progress bar: a light track with a brighter filled portion.
///
/// Unlike every other surface in this dark UI, both halves are deliberately
/// light. The status text sits *on* the bar and is black, so the unfilled track
/// has to carry black text just as readably as the filled part does — a dark
/// track would swallow the label for as long as the bar was near empty, which
/// is exactly when the user is most likely to be reading it.
pub fn progress_track(_theme: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        // Light grey rather than pure white, so the filled portion still reads
        // as distinct at a glance.
        background: Background::Color(Color::from_rgb(0.78, 0.78, 0.80)),
        // Desaturated green: clearly "progress", light enough for black text.
        bar: Background::Color(Color::from_rgb(0.45, 0.78, 0.52)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.55),
            width: 1.0,
            radius: 0.0.into(),
        },
    }
}

/// Thin border with no fill. Used to draw definitive box edges around
/// sub-elements within a zone (matches the foundation mockup).
pub fn bordered(_theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.4),
            width: 1.5,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Slightly darker fill + thicker border. Marks the center pane as the
/// menu-display surface (foundation mockup uses gray here).
pub fn menu_pane(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.55),
            width: 2.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn tab_active(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.18))),
        text_color: Color::WHITE,
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.6),
            width: 1.5,
            radius: 0.0.into(),
        },
        ..button::Style::default()
    }
}

pub fn tab_inactive(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::from_rgba(1.0, 1.0, 1.0, 0.7),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..button::Style::default()
    }
}
