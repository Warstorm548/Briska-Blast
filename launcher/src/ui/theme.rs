//! Shared sizing constants + container style helpers so the five zones
//! agree on dimensions and box boundaries.

use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

pub const ZONE_GAP: u32 = 4;
pub const RAIL_WIDTH: u32 = 220;
pub const BAR_HEIGHT: u32 = 48;
pub const TITLE_SIZE: u32 = 32;

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
