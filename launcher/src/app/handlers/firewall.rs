//! Windows-Firewall inbound-rule handling: the non-elevated status check
//! (Settings → Game Channel Management) and the first-Play Allow/Skip prompt
//! with its single-UAC elevated `netsh add rule`. Launch on accept/skip is
//! delegated to `crate::app::launch_game`.

use crate::app::{launch_game, AppState, CenterView, Message};
use crate::channel::Channel;
use iced::Task;

pub(crate) fn check_firewall(state: &mut AppState, channel: Channel) -> Task<Message> {
    // Defence-in-depth dev gate, matching VerifyChannel.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("CheckFirewall for Dev without dev_flag — refusing");
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        return Task::none();
    };
    let Some(install_dir) = creds.install_location.clone() else {
        tracing::debug!(?channel, "CheckFirewall with no install — no-op");
        return Task::none();
    };
    // Resolve the exact game exe (install_dir + manifest.executable),
    // then run the non-elevated firewall lookup against it. If the
    // manifest can't be read the rule target is unknown, so we surface
    // that as `Unknown` rather than guessing a path.
    Task::perform(
        async move {
            match crate::updater::branches::installed_manifest(&install_dir).await {
                Ok(Some(m)) => {
                    let exe = install_dir.join(&m.executable);
                    crate::firewall::inbound_rule_status(exe).await
                }
                Ok(None) => crate::firewall::FirewallStatus::Unknown(
                    "no installed.json — channel not installed".to_string(),
                ),
                Err(e) => crate::firewall::FirewallStatus::Unknown(e),
            }
        },
        move |status| Message::FirewallCheckComplete { channel, status },
    )
}

pub(crate) fn firewall_check_complete(
    state: &mut AppState,
    channel: Channel,
    status: crate::firewall::FirewallStatus,
) -> Task<Message> {
    tracing::info!(?channel, ?status, "firewall check complete");
    state.firewall_status.insert(channel, status);
    Task::none()
}

pub(crate) fn firewall_prompt_allow(state: &mut AppState) -> Task<Message> {
    let CenterView::FirewallPrompt {
        channel,
        exe,
        in_progress,
        ..
    } = &mut state.center_view
    else {
        return Task::none();
    };
    if *in_progress {
        return Task::none(); // already elevating — ignore double-click
    }
    *in_progress = true;
    let channel = *channel;
    let exe = exe.clone();
    // The elevated call is blocking (ShellExecuteExW + WaitForSingleObject),
    // so run it off the async executor via spawn_blocking.
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                crate::firewall::add_inbound_rule_elevated(channel, &exe)
            })
            .await
            .map_err(|e| format!("elevation task join error: {e}"))?
        },
        move |result| Message::FirewallRuleAddDone { channel, result },
    )
}

pub(crate) fn firewall_prompt_skip(state: &mut AppState) -> Task<Message> {
    let CenterView::FirewallPrompt {
        channel,
        install_dir,
        username,
        ..
    } = &state.center_view
    else {
        return Task::none();
    };
    let channel = *channel;
    let install_dir = install_dir.clone();
    let username = username.clone();
    // Suppress the prompt for this channel for the rest of the session.
    state.firewall_prompt_dismissed.insert(channel);
    state.center_view = CenterView::Default;
    tracing::info!(?channel, "firewall prompt skipped — launching without a rule");
    launch_game(state, channel, install_dir, username)
}

pub(crate) fn firewall_rule_add_done(
    state: &mut AppState,
    channel: Channel,
    result: Result<(), String>,
) -> Task<Message> {
    match result {
        Ok(()) => {
            tracing::info!(?channel, "inbound firewall rule created");
            // Rule now exists, so future Plays skip the prompt via detection.
            // Pull the launch params out of the prompt, then launch.
            if let CenterView::FirewallPrompt {
                install_dir,
                username,
                ..
            } = &state.center_view
            {
                let install_dir = install_dir.clone();
                let username = username.clone();
                state.center_view = CenterView::Default;
                return launch_game(state, channel, install_dir, username);
            }
            state.center_view = CenterView::Default;
        }
        Err(e) => {
            tracing::warn!(?channel, error = %e, "firewall rule creation failed");
            // Keep the prompt open so the user can retry or skip.
            if let CenterView::FirewallPrompt {
                in_progress, error, ..
            } = &mut state.center_view
            {
                *in_progress = false;
                *error = Some(e);
            }
        }
    }
    Task::none()
}
