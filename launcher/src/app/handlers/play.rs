//! The Play path: launch gating, the Windows first-Play firewall check, and
//! the game-exit handler. The actual spawn lives in `crate::app::launch_game`,
//! shared with the firewall-prompt outcomes in `super::firewall`.

use crate::app::{launch_game, AppState, CenterView, Message};
use crate::channel::Channel;
use iced::Task;

pub(crate) fn play_pressed(state: &mut AppState) -> Task<Message> {
    let channel = state.selected_channel;
    if state.game_running {
        tracing::debug!("game already running — ignoring PlayPressed");
        return Task::none();
    }
    if state.install_in_progress.is_some() {
        tracing::debug!(
            in_progress = ?state.install_in_progress,
            "install in flight — refusing to launch game"
        );
        return Task::none();
    }
    // Defence-in-depth dev gate. The channel selector hides Dev
    // when !dev_flag, but a stale UI state could still route here.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("PlayPressed for Dev without dev_flag — refusing");
        return Task::none();
    }
    let Some(creds) = state.identity.channels.get(&channel) else {
        tracing::warn!(?channel, "PlayPressed with no creds for channel — refusing");
        return Task::none();
    };
    // Same half-state guard as the button label uses: only proceed
    // when both install_location AND a parseable installed_version
    // are present.
    if creds.parsed_installed_version().is_none() {
        tracing::warn!(?channel, "PlayPressed without a complete install — refusing");
        return Task::none();
    }
    let Some(install_dir) = creds.install_location.clone() else {
        // Unreachable given parsed_installed_version succeeded, but
        // keep the explicit match so a future schema change can't
        // sneak past.
        return Task::none();
    };
    let username = state.identity.username.clone();

    // On Windows, gate the launch behind a one-time firewall check: if
    // no inbound rule exists for this channel's exe we prompt once
    // (option A) before launching. Skipped if the user already dismissed
    // the prompt for this channel this session. On other targets there
    // is nothing to add (outbound hole-punching), so launch directly.
    #[cfg(target_os = "windows")]
    if !state.firewall_prompt_dismissed.contains(&channel) {
        let dir = install_dir.clone();
        let user = username.clone();
        return Task::perform(
            async move {
                let (status, exe) =
                    match crate::updater::branches::installed_manifest(&dir).await {
                        Ok(Some(m)) => {
                            let exe = dir.join(&m.executable);
                            (
                                crate::firewall::inbound_rule_status(exe.clone()).await,
                                Some(exe),
                            )
                        }
                        Ok(None) => (
                            crate::firewall::FirewallStatus::Unknown(
                                "no installed.json — channel not installed".to_string(),
                            ),
                            None,
                        ),
                        Err(e) => (crate::firewall::FirewallStatus::Unknown(e), None),
                    };
                (status, exe, dir, user)
            },
            move |(status, exe, install_dir, username)| Message::PlayFirewallResolved {
                channel,
                status,
                exe,
                install_dir,
                username,
            },
        );
    }

    launch_game(state, channel, install_dir, username)
}

pub(crate) fn game_exited(
    state: &mut AppState,
    channel: Channel,
    result: Result<Option<i32>, String>,
) -> Task<Message> {
    state.game_running = false;
    match result {
        Ok(Some(code)) => tracing::info!(?channel, code, "game exited"),
        Ok(None) => tracing::info!(?channel, "game exited (signal)"),
        Err(e) => tracing::warn!(?channel, error = %e, "game spawn / wait failed"),
    }
    Task::none()
}

/// Only constructed on the Windows Play path (`PlayFirewallResolved`), so on
/// other targets this handler is reachable from the dispatcher but never
/// fired — matching the pre-refactor behaviour.
pub(crate) fn play_firewall_resolved(
    state: &mut AppState,
    channel: Channel,
    status: crate::firewall::FirewallStatus,
    exe: Option<std::path::PathBuf>,
    install_dir: std::path::PathBuf,
    username: String,
) -> Task<Message> {
    // Guard against a stale check: PlayPressed only blocks re-entry on
    // `game_running`, so a double-click during the async firewall check
    // can spawn two checks → two of these events. If the first already
    // launched the game, ignore the late one rather than double-launch
    // or reopen the prompt over a running game.
    if state.game_running {
        tracing::debug!(?channel, "PlayFirewallResolved ignored — game already running");
        return Task::none();
    }
    // Only prompt when we're confident the rule is missing AND we know
    // the exe to target. RulePresent / Unknown / NotApplicable, or an
    // unresolved exe, all launch directly — never block Play on an
    // inconclusive check.
    if matches!(status, crate::firewall::FirewallStatus::NotDetected) {
        if let Some(exe) = exe {
            tracing::info!(?channel, "no inbound rule — opening firewall prompt");
            state.center_view = CenterView::FirewallPrompt {
                channel,
                exe,
                install_dir,
                username,
                in_progress: false,
                error: None,
            };
            return Task::none();
        }
    }
    tracing::debug!(?channel, ?status, "firewall check inconclusive/present — launching");
    launch_game(state, channel, install_dir, username)
}
