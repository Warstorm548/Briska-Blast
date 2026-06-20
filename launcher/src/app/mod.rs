//! Top-level Iced state machine.
//! `AppState` (in `state`) is the view-model the UI reads from; `Message` (in
//! `message`) is the closed set of things the UI can ask to happen. The
//! per-feature `update` arms live in `handlers/*`; this module is the thin
//! dispatcher plus the shared helpers and lifecycle hooks (`boot`, `view`,
//! `theme`, `title`).

mod handlers;
mod message;
mod state;

pub use message::{CenterView, Message, SettingsTab};
pub use state::{AppState, ChannelUpdateStatus};

use handlers::{firewall, identity, install, launcher_update, maintenance, nav, play};

use crate::channel::Channel;
use crate::server_api;
use crate::ui;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::updater;
use iced::widget::{column, container, row};
use iced::{Element, Length, Task, Theme};
use shared::protocol::messages::RegisterRequest;

pub(crate) fn recompute_visible_channels(state: &mut AppState) {
    let mut v = vec![Channel::Stable, Channel::Ea];
    if state.dev_flag {
        v.push(Channel::Dev);
    }
    state.visible_channels = v;
}

pub(crate) fn register_request_for(state: &AppState, channel: Channel) -> RegisterRequest {
    let creds = state.identity.channels.get(&channel);
    RegisterRequest {
        username: state.identity.username.clone(),
        prior_player_id: creds.map(|c| c.player_id.clone()),
        prior_secret_token: creds.map(|c| c.secret_token.clone()),
    }
}

pub(crate) fn register_tasks(state: &AppState) -> Vec<Task<Message>> {
    let mut tasks = Vec::with_capacity(3);
    for channel in Channel::all() {
        let req = register_request_for(state, channel);
        tasks.push(Task::perform(
            server_api::register(channel, req),
            move |result| Message::RegisterDone { channel, result },
        ));
    }
    tasks
}

/// Boot-time GitHub Releases fan-out. One `latest_release` task per
/// **currently-visible** channel — Dev is added separately by the
/// RegisterDone(Dev, Ok) handler once `dev_flag` flips true, so unflagged
/// users never reach the GitHub API for the dev channel (foundation §3).
pub(crate) fn latest_release_tasks(state: &AppState) -> Vec<Task<Message>> {
    state
        .visible_channels
        .iter()
        .copied()
        .map(|channel| {
            Task::perform(
                crate::updater::branches::latest_release(channel),
                move |result| Message::LatestReleaseFetched { channel, result },
            )
        })
        .collect()
}

/// Derive `state.branch_updates_available` from real installed vs.
/// available versions. Filtered to `visible_channels` so the dev channel
/// never leaks into the top-bar banner for an unflagged user (foundation
/// §3 banner-filtering rule). Called from every handler that mutates
/// available_versions, installed_version, or visible_channels.
pub(crate) fn recompute_branch_updates_available(state: &mut AppState) {
    let mut updates = Vec::new();
    for channel in &state.visible_channels {
        let Some(creds) = state.identity.channels.get(channel) else {
            continue;
        };
        let Some(installed) = creds.parsed_installed_version() else {
            continue;
        };
        let Some(available) = state.available_versions.get(channel) else {
            continue;
        };
        if available > &installed {
            updates.push(*channel);
        }
    }
    state.branch_updates_available = updates;
}

/// Iced boot — produces initial state and spawns:
///   1. the GitHub Releases self-update check (always runs — GitHub-only,
///      no identity needed)
///   2. one /register call per channel — but ONLY when a non-empty username
///      is already on file. First-launch users see the welcome screen and
///      `ConfirmWelcomeUsername` kicks the /register fan-out instead.
pub fn boot() -> (AppState, Task<Message>) {
    // Layer-1 migration: anyone upgrading from a pre-installer build kept their
    // identity + saves next to the binary. Pull them into the per-user data
    // root before the first read so the account survives the move. Idempotent
    // and non-clobbering — see paths::migrate_legacy_data_if_needed.
    match crate::paths::migrate_legacy_data_if_needed() {
        Ok(true) => tracing::info!("migrated legacy launcher data into per-user data dir"),
        Ok(false) => {}
        Err(e) => tracing::warn!(
            error = %e,
            "legacy data migration failed; continuing with per-user data dir"
        ),
    }

    let loaded = match crate::identity::load() {
        Ok(Some(id)) => Some(id),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "identity load failed; falling back to first-run");
            None
        }
    };

    let mut state = AppState {
        update_check_in_flight: true,
        ..AppState::default()
    };
    if let Some(id) = loaded {
        state.identity = id;
    }

    // Cross-restart recovery: if a game this launcher spawned is still running
    // (the launcher was closed and reopened mid-game, so the in-memory child
    // handle is gone), reflect that so the game-gated buttons (Play, Change
    // Name, …) stay locked. The poll subscription clears it once that externally
    // owned process exits. A stale / crashed-game file is removed by `recover`.
    if let Some(rec) = crate::running_game::recover() {
        tracing::info!(
            channel = %rec.channel,
            pid = rec.pid,
            "recovered a still-running game on boot"
        );
        state.game_running = true;
        state.recovered_game = Some(rec);
    }

    let mut tasks: Vec<Task<Message>> = vec![Task::perform(
        updater::check_for_update(),
        Message::LauncherUpdateCheckDone,
    )];

    if state.identity.username.trim().is_empty() {
        // Gate the entire identity flow behind the welcome screen so the
        // server's first record of this user carries their chosen name.
        // The latest_release fan-out is held back too — we don't want to
        // populate available_versions before the user has seen the welcome
        // form, and Dev's fetch is gated on the dev_flag handshake anyway.
        state.awaiting_username = true;
    } else {
        tasks.extend(register_tasks(&state));
        tasks.extend(latest_release_tasks(&state));
    }

    (state, Task::batch(tasks))
}

pub fn update(state: &mut AppState, message: Message) -> Task<Message> {
    tracing::debug!(?message, "ui message received");
    match message {
        // ---- navigation / draft-field toggles ----
        Message::ChannelPicked(c) => nav::channel_picked(state, c),
        Message::OpenSettings => nav::open_settings(state),
        Message::ChangeNamePressed => nav::change_name_pressed(state),
        Message::LauncherUpdatePressed => nav::open_launcher_update(state),
        Message::CloseCenterMenu => nav::close_center_menu(state),
        Message::SettingsTabSelected(t) => nav::settings_tab_selected(state, t),
        Message::UsernameDraftChanged(s) => nav::username_draft_changed(state, s),
        Message::WelcomeDraftChanged(s) => nav::welcome_draft_changed(state, s),

        // ---- launcher self-update ----
        Message::CheckForUpdatesPressed => launcher_update::check_for_updates_pressed(state),
        Message::LauncherUpdateCheckDone(result) => {
            launcher_update::update_check_done(state, result)
        }
        Message::StartLauncherUpdatePressed => launcher_update::start_update_pressed(state),
        Message::SelfUpdateDone(result) => launcher_update::self_update_done(state, result),

        // ---- identity: register / username / welcome ----
        Message::RegisterDone { channel, result } => identity::register_done(state, channel, result),
        Message::UpdateUsernameDone { channel, result } => {
            identity::update_username_done(state, channel, result)
        }
        Message::ConfirmWelcomeUsername => identity::confirm_welcome_username(state),
        Message::ConfirmUsernameChange => identity::confirm_username_change(state),

        // ---- per-channel install pipeline ----
        Message::UpdatePressed => install::update_pressed(state),
        Message::InstallPromptLatestFetched { channel, result } => {
            install::install_prompt_latest_fetched(state, channel, result)
        }
        Message::PickInstallLocation => install::pick_install_location(state),
        Message::InstallLocationPicked(picked) => install::install_location_picked(state, picked),
        Message::InstallConfirmed => install::install_confirmed(state),
        Message::DownloadProgress { channel, progress } => {
            install::download_progress(state, channel, progress)
        }
        Message::InstallComplete { channel, result } => {
            install::install_complete(state, channel, result)
        }
        Message::LatestReleaseFetched { channel, result } => {
            install::latest_release_fetched(state, channel, result)
        }
        Message::CheckChannelUpdatePressed(channel) => {
            install::check_channel_update_pressed(state, channel)
        }
        Message::ChannelUpdateCheckDone { channel, result } => {
            install::channel_update_check_done(state, channel, result)
        }

        // ---- play / game launch ----
        Message::PlayPressed => play::play_pressed(state),
        Message::GameExited { channel, result } => play::game_exited(state, channel, result),
        Message::RecoveredGamePoll => play::recovered_game_poll(state),
        Message::PlayFirewallResolved {
            channel,
            status,
            exe,
            install_dir,
            username,
        } => play::play_firewall_resolved(state, channel, status, exe, install_dir, username),

        // ---- uninstall / verify / game saves ----
        Message::UninstallChannel(channel) => maintenance::uninstall_channel(state, channel),
        Message::UninstallKeepSavesToggled(v) => maintenance::uninstall_keep_saves_toggled(state, v),
        Message::UninstallConfirmed => maintenance::uninstall_confirmed(state),
        Message::UninstallComplete { channel, result } => {
            maintenance::uninstall_complete(state, channel, result)
        }
        Message::VerifyChannel(channel) => maintenance::verify_channel(state, channel),
        Message::VerifyComplete { channel, outcome } => {
            maintenance::verify_complete(state, channel, outcome)
        }
        Message::GameSavePressed(channel) => maintenance::game_save_pressed(state, channel),
        Message::GameSaveOpenDone { channel, result } => {
            maintenance::game_save_open_done(channel, result)
        }

        // ---- Windows firewall: check + first-Play prompt ----
        Message::CheckFirewall(channel) => firewall::check_firewall(state, channel),
        Message::FirewallCheckComplete { channel, status } => {
            firewall::firewall_check_complete(state, channel, status)
        }
        Message::FirewallPromptAllow => firewall::firewall_prompt_allow(state),
        Message::FirewallPromptSkip => firewall::firewall_prompt_skip(state),
        Message::FirewallRuleAddDone { channel, result } => {
            firewall::firewall_rule_add_done(state, channel, result)
        }
    }
}

/// Mark the game running and spawn it, wiring exit back to `GameExited`. Shared
/// by the direct Play path and every firewall-prompt outcome so the launch
/// logic lives in exactly one place.
pub(crate) fn launch_game(
    state: &mut AppState,
    channel: Channel,
    install_dir: std::path::PathBuf,
    username: String,
) -> Task<Message> {
    state.game_running = true;

    // The game authenticates to the server with this channel's identity, so
    // hand it the player_id + secret_token. Creds should always exist for an
    // installed channel (registration runs on first launch), but if the row
    // is somehow missing we still launch with empty creds — the game surfaces
    // the auth failure itself rather than the launcher silently refusing Play.
    let (player_id, secret_token) = match state.identity.channels.get(&channel) {
        Some(creds) => (creds.player_id.clone(), creds.secret_token.clone()),
        None => {
            tracing::warn!(?channel, "no identity creds for channel at launch — game will fail auth");
            (String::new(), String::new())
        }
    };
    let launcher_version = env!("CARGO_PKG_VERSION").to_string();

    Task::perform(
        crate::game_launch::spawn_and_wait(
            channel,
            install_dir,
            username,
            player_id,
            secret_token,
            launcher_version,
        ),
        move |result| Message::GameExited { channel, result },
    )
}

pub fn view(state: &AppState) -> Element<'_, Message> {
    if state.awaiting_username {
        return ui::welcome::view(state);
    }
    column![
        container(ui::top_bar::view(state)).height(Length::Fixed(BAR_HEIGHT as f32)),
        row![
            container(ui::left_rail::view(state)),
            container(ui::center::view(state)).width(Length::Fill),
            container(ui::right_rail::view(state)),
        ]
        .height(Length::Fill)
        .spacing(ZONE_GAP),
        container(ui::bottom_bar::view(state)).height(Length::Fixed(BAR_HEIGHT as f32)),
    ]
    .spacing(ZONE_GAP)
    .into()
}

/// The only background subscription: a slow poll active **only** while a game
/// was recovered from `running_game.json` on boot (launcher restarted mid-game).
/// It drives `RecoveredGamePoll` so the launcher notices when that externally
/// owned process finally exits and can re-enable the game-gated buttons. In the
/// normal case — and whenever idle — this is `Subscription::none()`, so there is
/// no recurring timer cost; the normal launch path reports exit via the
/// `spawn_and_wait` task's `GameExited` instead.
pub fn subscription(state: &AppState) -> iced::Subscription<Message> {
    if state.recovered_game.is_some() {
        iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::RecoveredGamePoll)
    } else {
        iced::Subscription::none()
    }
}

pub fn theme(_state: &AppState) -> Theme {
    Theme::Dark
}

pub fn title(_state: &AppState) -> String {
    String::from("BriskaBlast Launcher")
}
