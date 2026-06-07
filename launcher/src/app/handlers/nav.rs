//! Trivial center-panel / draft-field navigation toggles. Pure state writes,
//! no async work — every handler returns `Task::none()`.

use crate::app::{AppState, CenterView, Message, SettingsTab};
use crate::channel::Channel;
use iced::Task;

pub(crate) fn channel_picked(state: &mut AppState, c: Channel) -> Task<Message> {
    state.selected_channel = c;
    Task::none()
}

pub(crate) fn open_settings(state: &mut AppState) -> Task<Message> {
    state.center_view = CenterView::Settings {
        tab: SettingsTab::ChannelManagement,
    };
    Task::none()
}

pub(crate) fn change_name_pressed(state: &mut AppState) -> Task<Message> {
    state.center_view = CenterView::ChangeUsername {
        draft: state.identity.username.clone(),
    };
    Task::none()
}

pub(crate) fn open_launcher_update(state: &mut AppState) -> Task<Message> {
    state.center_view = CenterView::LauncherUpdate;
    Task::none()
}

pub(crate) fn close_center_menu(state: &mut AppState) -> Task<Message> {
    state.center_view = CenterView::Default;
    Task::none()
}

pub(crate) fn settings_tab_selected(state: &mut AppState, t: SettingsTab) -> Task<Message> {
    if let CenterView::Settings { tab } = &mut state.center_view {
        *tab = t;
    }
    Task::none()
}

pub(crate) fn username_draft_changed(state: &mut AppState, s: String) -> Task<Message> {
    if let CenterView::ChangeUsername { draft } = &mut state.center_view {
        *draft = s;
    }
    Task::none()
}

pub(crate) fn welcome_draft_changed(state: &mut AppState, s: String) -> Task<Message> {
    state.welcome_draft = s;
    Task::none()
}
