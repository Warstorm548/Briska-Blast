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
    let Some(creds) = state.identity.channels.get(&channel) else {
        return Task::none();
    };
    let Some(install_dir) = creds.install_location.clone() else {
        tracing::debug!(?channel, "VerifyChannel with no install — no-op");
        return Task::none();
    };
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
    state.verify_results.insert(channel, outcome);
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
