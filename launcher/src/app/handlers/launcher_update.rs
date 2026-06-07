//! Launcher self-update flow: the GitHub-Releases update check and the
//! rename-trick binary swap.

use crate::app::{AppState, Message};
use crate::updater::{self, UpdateCheckOutcome};
use iced::Task;

pub(crate) fn check_for_updates_pressed(state: &mut AppState) -> Task<Message> {
    if !state.update_check_in_flight && !state.self_update_in_flight {
        state.update_check_in_flight = true;
        state.last_self_update_error = None;
        return Task::perform(updater::check_for_update(), Message::LauncherUpdateCheckDone);
    }
    Task::none()
}

pub(crate) fn update_check_done(
    state: &mut AppState,
    result: Result<UpdateCheckOutcome, String>,
) -> Task<Message> {
    state.update_check_in_flight = false;
    match result {
        Ok(UpdateCheckOutcome::Available { version, notes }) => {
            state.launcher_update_available = true;
            state.launcher_available_version = version;
            state.launcher_release_notes = notes;
        }
        Ok(UpdateCheckOutcome::UpToDate) => {
            state.launcher_update_available = false;
            state.launcher_available_version.clear();
            state.launcher_release_notes.clear();
        }
        Err(e) => {
            tracing::warn!(error = %e, "launcher update check failed");
            state.last_self_update_error = Some(format!("Update check failed: {e}"));
        }
    }
    Task::none()
}

pub(crate) fn start_update_pressed(state: &mut AppState) -> Task<Message> {
    if state.game_running {
        tracing::warn!("refusing self-update: game is running");
        state.last_self_update_error = Some("Cannot update while the game is running.".into());
    } else if state.self_update_in_flight {
        tracing::debug!("self-update already in flight, ignoring");
    } else if state.update_check_in_flight {
        tracing::debug!("update check in flight, ignoring start press");
    } else if !state.launcher_update_available || state.launcher_available_version.is_empty() {
        tracing::debug!("no update available, ignoring start press");
    } else {
        state.self_update_in_flight = true;
        state.last_self_update_error = None;
        let version = state.launcher_available_version.clone();
        return Task::perform(updater::run_self_update(version), Message::SelfUpdateDone);
    }
    Task::none()
}

pub(crate) fn self_update_done(state: &mut AppState, result: Result<(), String>) -> Task<Message> {
    state.self_update_in_flight = false;
    match result {
        Ok(()) => {
            // Binary on disk has been swapped. Exit so the next launch
            // runs the new code; `self_update`'s rename-trick cleanup
            // happens on that next launch.
            tracing::info!("self-update succeeded — exiting for relaunch");
            std::process::exit(0);
        }
        Err(e) => {
            tracing::error!(error = %e, "self-update failed");
            state.last_self_update_error = Some(format!("Update failed: {e}"));
        }
    }
    Task::none()
}
