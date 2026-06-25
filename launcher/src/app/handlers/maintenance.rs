//! Stage 7 install maintenance: uninstall (with confirm prompt), file-integrity
//! verify, and opening the per-channel saves directory.

use crate::app::{recompute_branch_updates_available, AppState, CenterView, Message};
use crate::channel::Channel;
use crate::identity;
use iced::Task;

pub(crate) fn uninstall_channel(state: &mut AppState, channel: Channel) -> Task<Message> {
    // Per Stage 7 design: Uninstall is allowed regardless of
    // dev_flag so a previously-flagged user who got revoked can
    // still clean up orphan dev files. We do still need a
    // complete install row to act on, and we won't proceed while
    // an install is in flight, an uninstall is already running,
    // or the game is running.
    if state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
        || state.verify_in_progress.is_some()
    {
        tracing::debug!(?channel, "UninstallChannel refused — busy");
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        tracing::warn!(?channel, "UninstallChannel with no creds row — refusing");
        return Task::none();
    };
    let (Some(install_dir), Some(installed_version)) =
        (creds.install_location.clone(), creds.installed_version.clone())
    else {
        tracing::debug!(?channel, "UninstallChannel with no install on record — no-op");
        return Task::none();
    };
    state.center_view = CenterView::UninstallConfirm {
        channel,
        install_dir,
        installed_version,
        // Default Keep saves = true per foundation §2 — safer
        // default; the user has to explicitly opt into wiping.
        keep_saves: true,
        error: None,
    };
    Task::none()
}

pub(crate) fn uninstall_keep_saves_toggled(state: &mut AppState, v: bool) -> Task<Message> {
    if let CenterView::UninstallConfirm { keep_saves, .. } = &mut state.center_view {
        *keep_saves = v;
    }
    Task::none()
}

pub(crate) fn uninstall_confirmed(state: &mut AppState) -> Task<Message> {
    let (channel, install_dir, keep_saves) = if let CenterView::UninstallConfirm {
        channel,
        install_dir,
        keep_saves,
        ..
    } = &state.center_view
    {
        (*channel, install_dir.clone(), *keep_saves)
    } else {
        tracing::warn!("UninstallConfirmed without an active UninstallConfirm view");
        return Task::none();
    };
    // Re-check the busy gate on Confirm too — defence-in-depth
    // against a Confirm press that races a state change after
    // the prompt was opened.
    if state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
    {
        return Task::none();
    }
    // Record the in-flight uninstall BEFORE returning the task so
    // a fast second Confirm press (or any other Uninstall
    // affordance) is rejected by the busy gates above and in the
    // UI. Cleared by UninstallComplete.
    state.uninstall_in_progress = Some(channel);
    tracing::info!(?channel, keep_saves, "starting uninstall");
    Task::perform(
        crate::updater::branches::uninstall_install(install_dir, channel.dir_name(), keep_saves),
        move |result| Message::UninstallComplete { channel, result },
    )
}

pub(crate) fn uninstall_complete(
    state: &mut AppState,
    channel: Channel,
    result: Result<(), String>,
) -> Task<Message> {
    // Clear the in-flight flag unconditionally — success and
    // failure both end the spawned task.
    if state.uninstall_in_progress == Some(channel) {
        state.uninstall_in_progress = None;
    }
    match result {
        Ok(()) => {
            if let Some(creds) = state.identity.channels.get_mut(&channel) {
                creds.install_location = None;
                creds.installed_version = None;
            }
            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(
                    error = %e,
                    ?channel,
                    "failed to persist identity after uninstall"
                );
            }
            state.verify_results.remove(&channel);
            // Drop any cached firewall result — the exe it referred to is
            // gone, so a stale "rule present" must not linger.
            state.firewall_status.remove(&channel);
            recompute_branch_updates_available(state);
            state.center_view = CenterView::Default;
            tracing::info!(?channel, "uninstall complete");
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "uninstall failed");
            if let CenterView::UninstallConfirm {
                channel: pc, error, ..
            } = &mut state.center_view
            {
                if *pc == channel {
                    *error = Some(format!("Uninstall failed: {e}"));
                }
            }
        }
    }
    Task::none()
}

pub(crate) fn verify_channel(state: &mut AppState, channel: Channel) -> Task<Message> {
    // Defence-in-depth dev gate.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("VerifyChannel for Dev without dev_flag — refusing");
        return Task::none();
    }
    // Global single-flight: only one verify at a time (any channel). A second
    // verify can't be allowed to overwrite another's `verify_in_progress` slot
    // or have a late completion clear the wrong row's "Verifying…" state.
    if state.verify_in_progress.is_some() {
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        return Task::none();
    };
    let Some(install_dir) = creds.install_location.clone() else {
        tracing::debug!(?channel, "VerifyChannel with no install — no-op");
        return Task::none();
    };
    state.verify_in_progress = Some(channel);
    Task::perform(
        crate::updater::branches::verify_install(install_dir),
        move |outcome| Message::VerifyComplete { channel, outcome },
    )
}

pub(crate) fn verify_complete(
    state: &mut AppState,
    channel: Channel,
    outcome: crate::updater::branches::VerifyOutcome,
) -> Task<Message> {
    tracing::info!(?channel, ?outcome, "verify complete");
    if state.verify_in_progress == Some(channel) {
        state.verify_in_progress = None;
    }
    state.verify_results.insert(channel, outcome);
    Task::none()
}

/// Open the Repair confirmation prompt for an installed channel. Repair itself
/// (the fetch-by-tag reinstall) runs from `install::repair_confirmed`. Gated
/// like Uninstall: needs an install on record, refused while busy.
pub(crate) fn repair_channel(state: &mut AppState, channel: Channel) -> Task<Message> {
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("RepairChannel for Dev without dev_flag — refusing");
        return Task::none();
    }
    if state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
        || state.verify_in_progress.is_some()
    {
        tracing::debug!(?channel, "RepairChannel refused — busy");
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        return Task::none();
    };
    let Some(installed_version) = creds.installed_version.clone() else {
        tracing::debug!(?channel, "RepairChannel with no install on record — no-op");
        return Task::none();
    };
    state.center_view = CenterView::RepairConfirm {
        channel,
        version: installed_version,
        error: None,
    };
    Task::none()
}

/// Open the Reset Runtime Cache confirmation (Windows). The button is only
/// rendered on Windows, but the handler is platform-agnostic and gated like the
/// others. The cache is deleted from `reset_runtime_cache_confirmed`.
pub(crate) fn reset_cache_channel(state: &mut AppState, channel: Channel) -> Task<Message> {
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("ResetRuntimeCache for Dev without dev_flag — refusing");
        return Task::none();
    }
    // Can't reset the cache out from under a running game (it holds the files
    // open), and don't interrupt an install/uninstall.
    if state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
    {
        tracing::debug!(?channel, "ResetRuntimeCache refused — busy");
        return Task::none();
    }
    state.center_view = CenterView::ResetCacheConfirm {
        channel,
        error: None,
    };
    Task::none()
}

/// User confirmed Reset Runtime Cache — delete the cache so the game rebuilds
/// it on next launch. Re-checks the game-running gate (defence-in-depth).
pub(crate) fn reset_runtime_cache_confirmed(state: &mut AppState) -> Task<Message> {
    let channel = if let CenterView::ResetCacheConfirm { channel, .. } = &state.center_view {
        *channel
    } else {
        tracing::warn!("ResetRuntimeCacheConfirmed without an active prompt");
        return Task::none();
    };
    if state.game_running {
        if let CenterView::ResetCacheConfirm { error, .. } = &mut state.center_view {
            *error = Some("Close the game first, then reset the runtime cache.".to_string());
        }
        return Task::none();
    }
    // Block a double-press: a second delete racing the first against the same
    // folder can spuriously fail. Cleared by RuntimeCacheResetComplete.
    if state.reset_cache_in_progress.is_some() {
        return Task::none();
    }
    state.reset_cache_in_progress = Some(channel);
    tracing::info!(?channel, "resetting runtime cache");
    Task::perform(crate::paths::clear_runtime_cache(channel), move |result| {
        Message::RuntimeCacheResetComplete { channel, result }
    })
}

/// Runtime-cache delete finished. On success: return to channel management; on
/// failure: keep the prompt open with the error.
pub(crate) fn reset_runtime_cache_complete(
    state: &mut AppState,
    channel: Channel,
    result: Result<(), String>,
) -> Task<Message> {
    if state.reset_cache_in_progress == Some(channel) {
        state.reset_cache_in_progress = None;
    }
    match result {
        Ok(()) => {
            tracing::info!(?channel, "runtime cache reset — game will rebuild it on next launch");
            state.center_view = CenterView::Settings {
                tab: crate::app::SettingsTab::ChannelManagement,
            };
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "runtime cache reset failed");
            if let CenterView::ResetCacheConfirm { channel: pc, error } = &mut state.center_view {
                if *pc == channel {
                    *error = Some(format!("Reset failed: {e}"));
                }
            }
        }
    }
    Task::none()
}

pub(crate) fn game_save_pressed(state: &mut AppState, channel: Channel) -> Task<Message> {
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("GameSavePressed for Dev without dev_flag — refusing");
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        return Task::none();
    };
    let Some(install_dir) = creds.install_location.clone() else {
        tracing::debug!(?channel, "GameSavePressed with no install — no-op");
        return Task::none();
    };
    let saves_dir = install_dir.join("saves");
    Task::perform(
        async move {
            tokio::fs::create_dir_all(&saves_dir)
                .await
                .map_err(|e| format!("create saves dir: {e}"))?;
            let to_open = saves_dir.clone();
            tokio::task::spawn_blocking(move || open::that(&to_open))
                .await
                .map_err(|e| format!("open join: {e}"))?
                .map_err(|e| format!("open: {e}"))
        },
        move |result| Message::GameSaveOpenDone { channel, result },
    )
}

pub(crate) fn game_save_open_done(channel: Channel, result: Result<(), String>) -> Task<Message> {
    match result {
        Ok(()) => tracing::info!(?channel, "saves dir opened"),
        Err(e) => tracing::warn!(?channel, error = %e, "failed to open saves dir"),
    }
    Task::none()
}
