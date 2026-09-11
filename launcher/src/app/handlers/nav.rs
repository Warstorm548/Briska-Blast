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

/// The user picked a channel in the left-rail box. Applies it and remembers it
/// for the next launch.
///
/// This is the **only** write point for the channel memory, because
/// `Message::ChannelPicked` is emitted from exactly one place — the left-rail
/// `pick_list` (`ui/left_rail.rs`) — and only on a real user selection. While a
/// game is running that picker renders as a static label, so a mid-game switch
/// cannot reach here and cannot rewrite the file.
pub(crate) fn channel_picked(state: &mut AppState, c: Channel) -> Task<Message> {
    select_channel(state, c);
    // Persisted on EVERY pick, including one that does not move the selection.
    // A re-pick of the channel already showing is still the user answering the
    // question, and it can genuinely differ from what is on disk: boot shows
    // the fallback while a remembered Dev is parked, so a user who picks Stable
    // during that window is choosing Stable over a file that still says `dev`.
    // Skipping the write there would let the next launch snap back to Dev and
    // overrule them. Best-effort and synchronous — a few dozen bytes under the
    // per-user data root, and a failure is logged inside rather than surfaced.
    crate::preferences::save_selected_channel(c);
    Task::none()
}

/// Apply a channel selection to state.
///
/// Split from [`channel_picked`] so the two callers can differ on persistence:
/// a user pick writes the memory file, while the boot restore (the dev-flag
/// snap in `handlers::identity`) must NOT — it is applying what the file
/// already says, and rewriting it there would be a pointless disk touch. It
/// also keeps this transition unit-testable without writing to the real data
/// dir, which is what the tests below rely on.
pub(crate) fn select_channel(state: &mut AppState, c: Channel) {
    // Either way the boot restore is settled: this call is either the restore
    // itself, or the user reaching for the picker before the dev handshake
    // landed — and a deliberate pick must always beat a queued snap.
    state.pending_channel_restore = None;

    if c == state.selected_channel {
        return;
    }
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
    // The changelog pane is anchored at the *new* channel's installed
    // version, so its visible window is a different set of entries. Drop
    // the seeded open set to re-seed on the new top entry.
    state.changelog_open.remove(&crate::changelog::Kind::Game);
    state.selected_channel = c;
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
    // Returning to the changelog pane, whose window differs from whatever the
    // menu being closed was showing — re-seed so its top entry opens.
    state.changelog_open.remove(&crate::changelog::Kind::Game);
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

    // Every channel test below drives `select_channel`, never `channel_picked`.
    // `channel_picked` persists to the real per-user data dir, so calling it
    // from a unit test would overwrite the developer's own remembered channel
    // on each `cargo test`. The persistence is covered in `preferences.rs`.

    #[test]
    fn switching_channel_clears_update_verdict() {
        let mut state = AppState::default(); // selected = Stable
        state.channel_update_status.insert(
            Channel::Stable,
            ChannelUpdateStatus::UpToDate(Version::new(0, 12, 1)),
        );

        select_channel(&mut state, Channel::Ea);

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

        select_channel(&mut state, Channel::Ea);

        // The sentinel survives so the button stays deduped (no duplicate
        // request) and "Checking…" returns if the user switches back mid-flight.
        assert_eq!(
            state.channel_update_status.get(&Channel::Stable),
            Some(&ChannelUpdateStatus::Checking),
        );
    }

    /// A deliberate pick must cancel a queued boot restore, so a remembered
    /// Dev cannot yank the selection out from under a user who already chose
    /// something else while the dev handshake was still in flight.
    #[test]
    fn picking_a_channel_cancels_a_pending_restore() {
        let mut state = AppState {
            pending_channel_restore: Some(Channel::Dev),
            ..AppState::default() // selected = Stable
        };

        select_channel(&mut state, Channel::Ea);

        assert_eq!(state.selected_channel, Channel::Ea);
        assert_eq!(state.pending_channel_restore, None);
    }

    /// Even a no-op re-pick counts as the user settling the question — it is
    /// still a hand on the picker, so the queued snap is cancelled. This is the
    /// case that must also reach the disk: the selection does not move, but the
    /// file still says Dev, and `channel_picked` writes regardless.
    #[test]
    fn re_picking_the_same_channel_still_cancels_a_pending_restore() {
        let mut state = AppState {
            pending_channel_restore: Some(Channel::Dev),
            ..AppState::default() // selected = Stable
        };

        select_channel(&mut state, Channel::Stable);

        assert_eq!(state.selected_channel, Channel::Stable);
        assert_eq!(state.pending_channel_restore, None);
    }

    #[test]
    fn re_picking_same_channel_keeps_verdict() {
        let mut state = AppState::default(); // selected = Stable
        state
            .channel_update_status
            .insert(Channel::Stable, ChannelUpdateStatus::Checking);

        // No-op re-selection must not wipe an in-flight check.
        select_channel(&mut state, Channel::Stable);

        assert_eq!(
            state.channel_update_status.get(&Channel::Stable),
            Some(&ChannelUpdateStatus::Checking),
        );
    }
}
