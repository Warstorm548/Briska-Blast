//! Top-level Iced state machine.
//! `AppState` (in `state`) is the view-model the UI reads from; `Message` (in
//! `message`) is the closed set of things the UI can ask to happen. The
//! per-feature `update` arms live in `handlers/*`; this module is the thin
//! dispatcher plus the shared helpers and lifecycle hooks (`boot`, `view`,
//! `theme`, `title`).

/// `pub(crate)` so the view layer can reuse the changelog handler's shared
/// selection helpers (window size, channel filter, open-set seeding) rather
/// than re-deriving that logic per surface.
pub(crate) mod handlers;
mod message;
mod state;

pub use message::{CenterView, Message, SettingsTab};
pub use state::{ActiveUpdate, AppState, ChannelUpdateStatus};

use handlers::{
    changelog, firewall, identity, install, launcher_update, maintenance, nav, play,
};

use crate::channel::Channel;
use crate::preferences;
use crate::server_api;
use crate::ui;
use crate::ui::theme::{BAR_HEIGHT, ZONE_GAP};
use crate::updater;
use crate::updater::release_cache::Freshness;
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

/// Decide which channel a launch starts on, from the channel remembered in
/// `preferences.json` and the channels visible at that moment.
///
/// Returns `(selected, pending)`. A remembered channel that is currently
/// visible is selected outright. One that is not — in practice only Dev, which
/// stays hidden until the dev server's `/register` confirms the flag (§3 of the
/// foundation doc) — falls back to [`preferences::FALLBACK`] and is parked as
/// `pending` for the dev handshake to apply once the flag lands.
///
/// Pure on purpose: `boot()` itself reads the real data dir and spawns tasks,
/// so keeping the decision here is what makes it unit-testable.
fn resolve_boot_channel(remembered: Channel, visible: &[Channel]) -> (Channel, Option<Channel>) {
    if visible.contains(&remembered) {
        (remembered, None)
    } else {
        (preferences::FALLBACK, Some(remembered))
    }
}

/// Second half of the boot channel restore: settle a channel that was parked by
/// [`resolve_boot_channel`], now that the dev-flag handshake has resolved.
///
/// Called from **both** `RegisterDone { Dev, .. }` arms in `handlers::identity`.
/// On a confirmed flag the parked channel is now visible and gets selected; on a
/// denied flag or an unreachable dev server it is simply dropped, because a
/// restore that can no longer happen must not linger into a later handler and
/// move the selection unexpectedly.
pub(crate) fn apply_pending_channel_restore(state: &mut AppState) {
    let Some(pending) = state.pending_channel_restore.take() else {
        return;
    };
    if state.visible_channels.contains(&pending) {
        // Routed through the shared selection helper so the verdict-box reset
        // and changelog re-seed happen exactly as they would on a manual pick.
        // Deliberately NOT `channel_picked`: this is applying what the memory
        // file already says, so writing it back would be a pointless disk touch.
        handlers::nav::select_channel(state, pending);
        tracing::debug!(%pending, "restored remembered channel after the dev handshake");
    } else {
        tracing::debug!(
            %pending,
            "remembered channel is still not visible — staying on the fallback"
        );
    }
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
                // Cached: every channel here wants the same list within
                // milliseconds, so `release_cache` collapses the whole fan-out
                // (plus the self-update check below) into one request.
                crate::updater::branches::latest_release(channel, Freshness::Cached),
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

    // Channel memory: come up on whatever the user last picked rather than
    // always on Stable. `load_selected_channel` has already collapsed every
    // untrustworthy file into the fallback, so there is nothing to handle here.
    let (selected, pending) =
        resolve_boot_channel(preferences::load_selected_channel(), &state.visible_channels);
    state.selected_channel = selected;
    state.pending_channel_restore = pending;

    let mut tasks: Vec<Task<Message>> = vec![Task::perform(
        updater::check_for_update(Freshness::Cached),
        Message::LauncherUpdateCheckDone,
    )];

    // Cross-restart recovery: probe whether a game is already running (the
    // launcher was closed and reopened mid-game, so the in-memory child handle is
    // gone). On `true`, BootGameProbe locks the game-gated buttons and starts the
    // poll subscription that clears them once that externally owned process exits.
    // A crashed game leaves a stale `game_instance.json`, but the probe's connect
    // fails, so it reports not-running — no false lock.
    tasks.push(Task::perform(
        crate::rendezvous::game_is_running(),
        Message::BootGameProbe,
    ));

    // Changelog refresh + channel filter. Deliberately outside the
    // awaiting_username gate below: the two file fetches hit GitHub's raw CDN
    // (no rate-limit budget), and the filter reads the same release snapshot the
    // self-update check above already pays for, so none of this adds a counted
    // request or depends on identity.
    tasks.extend(changelog::boot_tasks());

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

        // ---- changelog viewer ----
        Message::ChangelogRefreshed { kind, result } => {
            changelog::refreshed(state, kind, result)
        }
        Message::ChangelogToggled { kind, version } => {
            changelog::toggled(state, kind, version)
        }
        Message::ChangelogShippedLoaded(result) => changelog::shipped_loaded(state, result),
        Message::OpenUrl(url) => changelog::open_url(url),

        // ---- launcher self-update ----
        Message::CheckForUpdatesPressed => launcher_update::check_for_updates_pressed(state),
        Message::LauncherUpdateCheckDone(result) => {
            launcher_update::update_check_done(state, result)
        }
        Message::StartLauncherUpdatePressed => launcher_update::start_update_pressed(state),
        Message::SelfUpdateProgress(progress) => {
            launcher_update::self_update_progress(state, progress)
        }
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
        Message::UpdatePlanned { channel, plan } => {
            install::update_planned(state, channel, plan)
        }
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
        Message::BootGameProbe(running) => play::boot_game_probe(state, running),
        Message::RecoveredGamePoll => play::recovered_game_poll(state),
        Message::RecoveredGameProbe(running) => play::recovered_game_probe(state, running),
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
        Message::RepairChannel(channel) => maintenance::repair_channel(state, channel),
        Message::RepairConfirmed => install::repair_confirmed(state),
        Message::RepairComplete { channel, result } => {
            install::repair_complete(state, channel, result)
        }
        Message::ResetRuntimeCache(channel) => maintenance::reset_cache_channel(state, channel),
        Message::ResetRuntimeCacheConfirmed => maintenance::reset_runtime_cache_confirmed(state),
        Message::RuntimeCacheResetComplete { channel, result } => {
            maintenance::reset_runtime_cache_complete(state, channel, result)
        }
        Message::GameSavePressed(channel) => maintenance::game_save_pressed(state, channel),
        Message::GameSaveOpenDone { channel, result } => {
            maintenance::game_save_open_done(channel, result)
        }
        Message::GameLogsPressed(channel) => maintenance::game_logs_pressed(state, channel),
        Message::GameLogsOpenDone { channel, result } => {
            maintenance::game_logs_open_done(channel, result)
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
/// was recovered by the boot liveness probe (launcher restarted mid-game). It
/// drives `RecoveredGamePoll` so the launcher notices when that externally owned
/// process finally exits and can re-enable the game-gated buttons. In the normal
/// case — and whenever idle — this is `Subscription::none()`, so there is no
/// recurring timer cost; the normal launch path reports exit via the
/// `spawn_and_wait` task's `GameExited` instead.
pub fn subscription(state: &AppState) -> iced::Subscription<Message> {
    if state.recovered_game_running {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A remembered channel that is already visible is restored as-is, with
    /// nothing left pending.
    #[test]
    fn visible_remembered_channel_is_restored_directly() {
        let visible = [Channel::Stable, Channel::Ea];
        assert_eq!(
            resolve_boot_channel(Channel::Ea, &visible),
            (Channel::Ea, None)
        );
        assert_eq!(
            resolve_boot_channel(Channel::Stable, &visible),
            (Channel::Stable, None)
        );
    }

    /// Dev is hidden at boot on every launch, flagged user or not, because the
    /// flag only arrives with the dev `/register` response. It must therefore
    /// start on the fallback and park Dev for the handshake to pick up.
    #[test]
    fn hidden_remembered_channel_is_parked_as_pending() {
        let visible = [Channel::Stable, Channel::Ea];
        assert_eq!(
            resolve_boot_channel(Channel::Dev, &visible),
            (preferences::FALLBACK, Some(Channel::Dev))
        );
    }

    /// Once Dev is visible — the path a *second* resolution would take — it is
    /// restored outright with nothing pending.
    #[test]
    fn dev_is_restored_directly_when_already_visible() {
        let visible = [Channel::Stable, Channel::Ea, Channel::Dev];
        assert_eq!(
            resolve_boot_channel(Channel::Dev, &visible),
            (Channel::Dev, None)
        );
    }

    /// The flag confirmed: Dev is now visible, so the parked selection applies.
    #[test]
    fn pending_restore_applies_once_the_channel_is_visible() {
        let mut state = AppState {
            pending_channel_restore: Some(Channel::Dev),
            dev_flag: true,
            ..AppState::default() // selected = Stable
        };
        recompute_visible_channels(&mut state);

        apply_pending_channel_restore(&mut state);

        assert_eq!(state.selected_channel, Channel::Dev);
        assert_eq!(state.pending_channel_restore, None);
    }

    /// The flag was denied, or the dev server was unreachable. The selection
    /// stays on the fallback and the park is dropped, so it cannot fire later.
    #[test]
    fn pending_restore_is_dropped_when_the_channel_stays_hidden() {
        let mut state = AppState {
            pending_channel_restore: Some(Channel::Dev),
            ..AppState::default() // selected = Stable, dev_flag false
        };

        apply_pending_channel_restore(&mut state);

        assert_eq!(state.selected_channel, preferences::FALLBACK);
        assert_eq!(state.pending_channel_restore, None);
    }

    /// The common case — Stable or EA restored at boot, nothing parked — must
    /// leave the user's current selection completely alone.
    #[test]
    fn no_pending_restore_leaves_the_selection_untouched() {
        let mut state = AppState {
            selected_channel: Channel::Ea,
            dev_flag: true,
            ..AppState::default()
        };
        recompute_visible_channels(&mut state);

        apply_pending_channel_restore(&mut state);

        assert_eq!(state.selected_channel, Channel::Ea);
    }
}
