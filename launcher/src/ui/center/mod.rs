//! Center pane router. Dispatches to one of three views based on
//! `AppState::center_view`. See docs/launcher/launcher-foundation.md §1.

use crate::app::{AppState, CenterView, Message};
use iced::Element;

pub mod change_username;
pub mod default;
pub mod install_prompt;
pub mod launcher_update;
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
    }
}
