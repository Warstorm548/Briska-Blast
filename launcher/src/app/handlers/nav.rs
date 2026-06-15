//! Trivial center-panel / draft-field navigation toggles. Pure state writes,
//! no async work — every handler returns `Task::none()`.

use crate::app::{AppState, CenterView, Message, SettingsTab};
use crate::channel::Channel;
use iced::Task;

pub(crate) fn channel_picked(state: &mut AppState, c: Channel) -> Task<Message> {
    if c != state.selected_channel {
        // Drop the manual update-check verdict so the left-rail box starts
        // fresh (em-dash) for the newly-focused channel. `available_versions`
        // is intentionally left intact — the bottom-bar Update button keeps
        // its per-channel state across switches.
        state.channel_update_status.clear();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ChannelUpdateStatus;
    use semver::Version;

    #[test]
    fn switching_channel_clears_update_verdict() {
        let mut state = AppState::default(); // selected = Stable
        state.channel_update_status.insert(
            Channel::Stable,
            ChannelUpdateStatus::UpToDate(Version::new(0, 12, 1)),
        );

        let _ = channel_picked(&mut state, Channel::Ea);

        assert_eq!(state.selected_channel, Channel::Ea);
        assert!(
            state.channel_update_status.is_empty(),
            "verdict box must reset when the focused channel changes"
        );
    }

    #[test]
    fn re_picking_same_channel_keeps_verdict() {
        let mut state = AppState::default(); // selected = Stable
        state
            .channel_update_status
            .insert(Channel::Stable, ChannelUpdateStatus::Checking);

        // No-op re-selection must not wipe an in-flight check.
        let _ = channel_picked(&mut state, Channel::Stable);

        assert_eq!(
            state.channel_update_status.get(&Channel::Stable),
            Some(&ChannelUpdateStatus::Checking),
        );
    }
}
