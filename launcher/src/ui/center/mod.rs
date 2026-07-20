//! Center pane router. Dispatches to one of three views based on
//! `AppState::center_view`. See docs/launcher/launcher-foundation.md §1.

use crate::app::{AppState, CenterView, Message};
use iced::widget::scrollable;
use iced::{Element, Length};

pub mod change_username;
pub mod default;
pub mod firewall_prompt;
pub mod install_prompt;
pub mod launcher_update;
pub mod repair_confirm;
pub mod reset_cache_confirm;
pub mod settings;
pub mod uninstall_confirm;

pub fn view(state: &AppState) -> Element<'_, Message> {
    match &state.center_view {
        CenterView::Default => default::view(state),
        CenterView::Settings { tab } => settings::view(state, *tab),
        CenterView::ChangeUsername { draft } => change_username::view(state, draft),
        CenterView::LauncherUpdate => launcher_update::view(state),
        CenterView::InstallPrompt { .. } => install_prompt::dispatch(state, &state.center_view),
        CenterView::UninstallConfirm { .. } => {
            uninstall_confirm::dispatch(state, &state.center_view)
        }
        CenterView::RepairConfirm { .. } => repair_confirm::dispatch(state, &state.center_view),
        CenterView::ResetCacheConfirm { .. } => {
            reset_cache_confirm::dispatch(state, &state.center_view)
        }
        CenterView::FirewallPrompt { .. } => {
            firewall_prompt::dispatch(state, &state.center_view)
        }
    }
}

/// Wrap a center view's scrolling body so it fills the remaining pane height and
/// shows a vertical scrollbar only when the content overflows. Mouse-wheel
/// scrolling works when the pointer is over it (Iced built-in). The view's
/// header/tab bar stays pinned because it is a *sibling* of this element in the
/// view's `column!`, not inside it.
///
/// Only the height is pinned to `Fill` (the bounded viewport that makes overflow
/// detectable). Width is left to Iced's `enclose`, which ties it to the content's
/// own width — so a full-width body (Settings) gets a far-right scrollbar while
/// the fixed-width confirm dialogs keep their existing width/placement.
pub fn scroll_area<'a>(body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(body).height(Length::Fill).into()
}
