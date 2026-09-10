//! Shared changelog accordion.
//!
//! One widget behind all four surfaces — the per-channel default center view,
//! the game update prompt, the Launcher Update view, and the Launcher Changelog
//! settings tab — so they cannot drift apart in look or behaviour.
//!
//! Follows the `super::launcher_update::content` template: return a bare
//! element with no `menu_pane` and no header, and let each caller's
//! `super::scroll_area` supply the viewport. Per the note in `settings.rs`, a
//! `Length::Fill` height inside a scroll-area body is undefined, so everything
//! here sizes itself by content.

use crate::app::Message;
use crate::changelog::{Kind, Section};
use crate::ui::theme::{self, ZONE_GAP};
use iced::widget::{button, column, container, markdown, row, text, Space};
use iced::{Alignment, Element, Length};
use std::collections::HashSet;

/// Body text size, matching the explainer copy elsewhere in Settings.
const BODY_SIZE: u32 = 13;
const HEADING_SIZE: u32 = 15;

/// Render `entries` as an accordion plus a "full changelog" link.
///
/// `open` is the set of expanded versions; `empty_note` is shown when there is
/// nothing to list (a channel whose filter excluded everything, say).
pub fn view<'a>(
    kind: Kind,
    entries: &[&'a Section],
    open: &HashSet<semver::Version>,
    empty_note: &'a str,
) -> Element<'a, Message> {
    let mut col = column![].spacing(ZONE_GAP * 2);

    if entries.is_empty() {
        col = col.push(text(empty_note).size(BODY_SIZE));
    } else {
        for section in entries {
            col = col.push(entry_row(kind, section, open.contains(section.version())));
        }
    }

    col = col.push(
        button(text("View the full changelog on GitHub").size(BODY_SIZE))
            .on_press(Message::OpenUrl(kind.web_url()))
            .padding(8),
    );

    col.into()
}

/// One collapsible entry: a full-width header button, and the rendered markdown
/// body when expanded.
fn entry_row<'a>(kind: Kind, section: &'a Section, is_open: bool) -> Element<'a, Message> {
    // A caret rather than a coloured cue, so the open/closed state does not
    // depend on the user distinguishing colours.
    let caret = if is_open { "\u{25BE}" } else { "\u{25B8}" };
    let date: Element<'a, Message> = if section.entry.date.is_empty() {
        Space::new().width(Length::Shrink).into()
    } else {
        text(section.entry.date.as_str()).size(BODY_SIZE).into()
    };

    let header = button(
        row![
            text(format!("{caret}  v{}", section.version())).size(HEADING_SIZE),
            Space::new().width(Length::Fill),
            date,
        ]
        .align_y(Alignment::Center),
    )
    .on_press(Message::ChangelogToggled {
        kind,
        version: section.version().clone(),
    })
    .width(Length::Fill)
    .padding(8);

    let mut cell = column![header].spacing(ZONE_GAP);
    if is_open {
        cell = cell.push(container(body(section)).padding(8));
    }

    container(cell)
        .style(theme::bordered)
        .width(Length::Fill)
        .into()
}

/// Render one entry's pre-parsed markdown.
///
/// The items are borrowed from `Section`, not parsed here: `markdown::view`
/// returns an `Element` that borrows them, so producing them locally would not
/// outlive the return. `markdown::view`'s message type is the clicked link's
/// URI, mapped onto `OpenUrl` so a link inside an entry behaves like the
/// "full changelog" button.
fn body(section: &Section) -> Element<'_, Message> {
    // The app's theme is fixed to Dark (`crate::app::theme`), so the markdown
    // palette is derived from the same theme the rest of the UI is drawn with.
    let settings =
        markdown::Settings::with_text_size(BODY_SIZE, markdown::Style::from(&iced::Theme::Dark));
    markdown::view(&section.md, settings)
        .map(|uri| Message::OpenUrl(uri.to_string()))
}
