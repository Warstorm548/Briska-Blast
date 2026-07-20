//! Identity lifecycle: `/register` responses, username changes (live + the
//! 401 self-heal re-register), and the first-launch welcome-screen confirm.

use crate::app::{
    latest_release_tasks, recompute_branch_updates_available, recompute_visible_channels,
    register_request_for, register_tasks, AppState, CenterView, Message,
};
use crate::channel::Channel;
use crate::identity::{self, ChannelCreds};
use crate::server_api;
use iced::Task;
use shared::protocol::messages::{RegisterResponse, UpdateUsernameRequest, MAX_USERNAME_LEN};

pub(crate) fn register_done(
    state: &mut AppState,
    channel: Channel,
    result: Result<RegisterResponse, String>,
) -> Task<Message> {
    match result {
        Ok(resp) => {
            // Preserve install_location / installed_version if this
            // channel was already installed. /register only refreshes
            // identity creds; it must not wipe Stage 3 install state.
            let prior = state.identity.channels.remove(&channel);
            let mut creds = ChannelCreds::from_register(resp.player_id, resp.secret_token);
            if let Some(p) = prior {
                creds.install_location = p.install_location;
                creds.installed_version = p.installed_version;
            }
            state.identity.channels.insert(channel, creds);
            // Server is canonical for username; reflect any drift back
            // into state.identity.
            state.identity.username = resp.username;
            state.server_reachable.insert(channel, true);

            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(
                    error = %e,
                    channel = %channel,
                    "failed to persist identity after register"
                );
            }

            if channel == Channel::Dev {
                state.dev_flag = resp.dev_flag;
                recompute_visible_channels(state);
                if resp.dev_flag {
                    // Dev just became visible — fetch its latest
                    // release now (boot's fan-out skipped Dev when
                    // visible_channels was Stable+Ea only).
                    recompute_branch_updates_available(state);
                    return Task::perform(
                        crate::updater::branches::latest_release(Channel::Dev),
                        |result| Message::LatestReleaseFetched {
                            channel: Channel::Dev,
                            result,
                        },
                    );
                } else {
                    // Dev hidden — drop any cached version so the
                    // banner doesn't dangle from a prior flagged
                    // run; rebuild the derived banner.
                    state.available_versions.remove(&Channel::Dev);
                    recompute_branch_updates_available(state);
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, channel = %channel, "register failed");
            state.server_reachable.insert(channel, false);
            if channel == Channel::Dev {
                // Dev server unreachable → must hide dev row, per
                // the foundation visibility matrix (row "No / — /
                // (unknowable) / No").
                state.dev_flag = false;
                recompute_visible_channels(state);
                state.available_versions.remove(&Channel::Dev);
                recompute_branch_updates_available(state);
            }
        }
    }
    Task::none()
}

pub(crate) fn update_username_done(
    state: &mut AppState,
    channel: Channel,
    result: Result<server_api::UpdateUsernameOutcome, server_api::ServerApiError>,
) -> Task<Message> {
    match result {
        Ok(server_api::UpdateUsernameOutcome::Accepted(name)) => {
            // Server stored the new name and echoes it back as canonical. Adopt
            // it (normally identical to the value we set optimistically on
            // Confirm); persist only when it actually differs so the per-channel
            // fan-out doesn't rewrite identity.json once per channel.
            if state.identity.username != name {
                state.identity.username = name;
                if let Err(e) = identity::save(&state.identity) {
                    tracing::warn!(
                        error = %e,
                        channel = %channel,
                        "failed to persist identity after username accept"
                    );
                }
            }
        }
        Ok(server_api::UpdateUsernameOutcome::Rejected(stable)) => {
            // Trust-boundary reject. The launcher hard-caps the input, so a
            // reject here means a tampered/raw client; the server left Redis
            // untouched and handed back the stored stable name. Snap the local
            // identity back to it and surface a notice.
            tracing::warn!(
                channel = %channel,
                "username change rejected by server — reverting to stored name"
            );
            state.identity.username = stable;
            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(
                    error = %e,
                    channel = %channel,
                    "failed to persist identity after username revert"
                );
            }
            state.username_notice =
                Some("Server rejected that name; reverted to your saved username.".to_string());
        }
        Err(server_api::ServerApiError::Unauthorized) => {
            // The server no longer recognises this identity — it was deleted
            // (and possibly its id recycled to someone else) via the admin
            // panel. Re-register this channel: /register rejects the stale
            // creds and issues fresh ones (a recycled id from the pool), and
            // the RegisterDone(Ok) handler persists them and re-applies the
            // username. One round-trip fully heals the channel.
            tracing::warn!(
                channel = %channel,
                "update_username unauthorized — re-registering identity"
            );
            let req = register_request_for(state, channel);
            return Task::perform(server_api::register(channel, req), move |result| {
                Message::RegisterDone { channel, result }
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, channel = %channel, "update_username failed");
        }
    }
    Task::none()
}

pub(crate) fn confirm_welcome_username(state: &mut AppState) -> Task<Message> {
    let trimmed = state.welcome_draft.trim().to_string();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_USERNAME_LEN {
        // Defence in depth — the Confirm button is disabled when empty and the
        // input is hard-capped at MAX_USERNAME_LEN, but `on_submit` (Enter key)
        // routes here too.
        return Task::none();
    }

    // Build a candidate and persist BEFORE mutating shared state, so
    // a save failure leaves both the on-disk file and AppState in
    // their pre-Confirm form. The welcome screen stays up and the
    // typed text in welcome_draft is preserved so the user can retry
    // without retyping. We must NOT proceed to /register on a save
    // failure: otherwise the server would record an identity we have
    // no on-disk record of, and the next boot would issue a fresh
    // player_id (different from the one already on the server).
    let mut candidate = state.identity.clone();
    candidate.username = trimmed;
    if let Err(e) = identity::save(&candidate) {
        tracing::warn!(
            error = %e,
            "failed to save initial identity; staying on welcome screen"
        );
        return Task::none();
    }

    state.identity = candidate;
    state.welcome_draft.clear();
    state.awaiting_username = false;
    let mut tasks = register_tasks(state);
    tasks.extend(latest_release_tasks(state));
    Task::batch(tasks)
}

pub(crate) fn confirm_username_change(state: &mut AppState) -> Task<Message> {
    if let CenterView::ChangeUsername { draft } = &state.center_view {
        let trimmed = draft.trim().to_string();
        // The input is hard-capped at MAX_USERNAME_LEN, so the length re-check is
        // pure defence-in-depth (Enter-key path); empty stays a no-op.
        if !trimmed.is_empty() && trimmed.chars().count() <= MAX_USERNAME_LEN {
            state.username_notice = None;
            state.identity.username = trimmed.clone();
            state.center_view = CenterView::Default;
            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(error = %e, "failed to persist identity after rename");
            }
            // Tell every channel server the launcher already has
            // credentials for — fire-and-forget; failures are logged
            // but don't block the UI.
            let mut tasks: Vec<Task<Message>> = Vec::new();
            for (channel, creds) in &state.identity.channels {
                let req = UpdateUsernameRequest {
                    player_id: creds.player_id.clone(),
                    secret_token: creds.secret_token.clone(),
                    username: trimmed.clone(),
                };
                let ch = *channel;
                tasks.push(Task::perform(
                    server_api::update_username(ch, req),
                    move |result| Message::UpdateUsernameDone { channel: ch, result },
                ));
            }
            if !tasks.is_empty() {
                return Task::batch(tasks);
            }
        }
    }
    Task::none()
}
