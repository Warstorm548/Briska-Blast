//! Trivial center-panel / draft-field navigation toggles. Pure state writes,
//! no async work — every handler returns `Task::none()`.

use crate::app::{AppState, CenterView, ChannelUpdateStatus, Message, SettingsTab};
use crate::channel::Channel;
use iced::Task;
use shared::protocol::messages::MAX_USERNAME_LEN;

/// Hard-cap a username draft at `MAX_USERNAME_LEN` Unicode scalar values. Used by
/// both username inputs so a 21st character is never even stored in the draft —
/// the launcher therefore never sends an over-length name to the server.
fn cap_username(s: &str) -> String {
    s.chars().take(MAX_USERNAME_LEN).collect()
}

pub(crate) fn channel_picked(state: &mut AppState, c: Channel) -> Task<Message> {
    if c != state.selected_channel {
        // Reset the verdict box for the newly-focused channel: drop completed
        // verdicts so it shows the em-dash, but keep any in-flight `Checking`
        // sentinel. Dropping that sentinel would re-enable the channel's button
        // (its dedup guard keys off `Checking`), allow a duplicate GitHub
        // request, and lose the "Checking…" indicator if the user switches back
        // mid-flight. `available_versions` is left intact so the bottom-bar
        // Update button keeps its per-channel state across switches.
        state
            .channel_update_status
            .retain(|_, status| matches!(status, ChannelUpdateStatus::Checking));
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
    if state.game_running {
        // Username is locked while a game is running — the in-game identity must
        // not change mid-session. The button is disabled too; this guards the
        // stale-press path.
        tracing::debug!("ChangeNamePressed ignored — game running");
        return Task::none();
    }
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
        *draft = cap_username(&s);
    }
    Task::none()
}

pub(crate) fn welcome_draft_changed(state: &mut AppState, s: String) -> Task<Message> {
    state.welcome_draft = cap_username(&s);
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ChannelUpdateStatus;
    use semver::Version;

    #[test]
    fn cap_username_enforces_scalar_value_limit() {
        // Over-limit ASCII is truncated to exactly MAX_USERNAME_LEN.
        let long = "x".repeat(MAX_USERNAME_LEN + 5);
        assert_eq!(cap_username(&long).chars().count(), MAX_USERNAME_LEN);
        // Under-limit passes through unchanged.
        assert_eq!(cap_username("alice"), "alice");
        // Counts scalar values, not bytes — multibyte chars each count once.
        let emoji = "🙂".repeat(MAX_USERNAME_LEN + 2);
        assert_eq!(cap_username(&emoji).chars().count(), MAX_USERNAME_LEN);
    }

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
    fn switching_channel_keeps_in_flight_check() {
        let mut state = AppState::default(); // selected = Stable
        // A check is in flight on the channel we're about to leave.
        state
            .channel_update_status
            .insert(Channel::Stable, ChannelUpdateStatus::Checking);

        let _ = channel_picked(&mut state, Channel::Ea);

        // The sentinel survives so the button stays deduped (no duplicate
        // request) and "Checking…" returns if the user switches back mid-flight.
        assert_eq!(
            state.channel_update_status.get(&Channel::Stable),
            Some(&ChannelUpdateStatus::Checking),
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
