//! Per-channel game install / update pipeline: the install prompt, the
//! folder picker, the streamed download+extract, and the boot-time
//! `latest_release` fan-out that feeds the bottom-left button + update banner.

use crate::app::{recompute_branch_updates_available, AppState, CenterView, Message};
use crate::channel::Channel;
use crate::identity;
use futures_util::StreamExt;
use iced::Task;

/// Shown in the install prompt when the chosen folder collides with the
/// launcher's own install directory (see `paths::install_location_collides`).
/// Installing the game there leaves it unable to launch or obtain its Windows
/// firewall rule, so the install is refused rather than silently broken.
const INVALID_INSTALL_LOCATION_MSG: &str =
    "Invalid install location: that folder is inside the launcher's own application \
     folder. The game can't run or get its firewall permission from there. Please \
     choose a different location (e.g. your Documents or a Games folder).";

/// Internal pipe between the spawned download task and the Iced stream
/// adapter — see `install_confirmed`. Not surfaced as a public Message
/// because the variants here are mapped 1:1 onto two existing user-facing
/// Messages (DownloadProgress, InstallComplete).
enum InstallStreamEvent {
    Progress(crate::updater::branches::InstallProgress),
    Complete(Result<crate::updater::branches::InstallResult, String>),
}

pub(crate) fn update_pressed(state: &mut AppState) -> Task<Message> {
    let channel = state.selected_channel;
    // Defence-in-depth: the channel selector hides Dev when
    // !dev_flag, so this should be unreachable in practice. Logged
    // and refused if it ever fires.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("UpdatePressed for Dev without dev_flag — refusing");
        return Task::none();
    }
    if state.install_in_progress.is_some() {
        tracing::debug!(
            in_progress = ?state.install_in_progress,
            "install already in flight — ignoring"
        );
        return Task::none();
    }
    // Resolve the two pieces of state the button label is also
    // driven by, so the action taken matches what the user clicked.
    let creds = state.identity.channels.get(&channel);
    let installed_version = creds.and_then(|c| c.parsed_installed_version());
    let install_root_prior = creds.and_then(|c| {
        c.install_location.as_ref().map(|p| {
            // The install_location stored in identity.json is the
            // resolved <root>/<channel>/ path; the prompt re-joins
            // the channel dir, so we strip it back to the root.
            p.parent().map(|pp| pp.to_path_buf()).unwrap_or_else(|| p.clone())
        })
    });
    let available_str = state
        .available_versions
        .get(&channel)
        .map(|v| v.to_string());
    // Decide what action to take. Mirrors the bottom-left button
    // state machine — the button is disabled in every "no action"
    // branch, so reaching them here means the state changed under
    // us (e.g. a late LatestReleaseFetched).
    let (install_root, action_label): (Option<std::path::PathBuf>, &'static str) =
        match (installed_version.as_ref(), state.available_versions.get(&channel)) {
            // Update flow — same install_root, new version.
            (Some(inst), Some(avail)) if avail > inst => (install_root_prior, "update"),
            // Fresh install — user picks the install_root next.
            (None, Some(_)) => (None, "install"),
            _ => {
                tracing::debug!(
                    ?channel,
                    installed = ?installed_version,
                    available = ?available_str,
                    "UpdatePressed with no actionable state — ignoring"
                );
                return Task::none();
            }
        };
    tracing::debug!(?channel, action = %action_label, "opening install prompt");
    state.center_view = CenterView::InstallPrompt {
        channel,
        install_root,
        available: available_str.clone(),
        error: None,
    };
    // Cache hit is the normal case; fall back to an in-prompt fetch
    // only when the cache is empty (rare — button is disabled then).
    if available_str.is_none() {
        return Task::perform(
            crate::updater::branches::latest_release(channel),
            move |result| Message::InstallPromptLatestFetched { channel, result },
        );
    }
    Task::none()
}

pub(crate) fn install_prompt_latest_fetched(
    state: &mut AppState,
    channel: Channel,
    result: Result<Option<crate::updater::branches::GameRelease>, String>,
) -> Task<Message> {
    if let CenterView::InstallPrompt {
        channel: pc,
        available,
        error,
        ..
    } = &mut state.center_view
    {
        // Guard against a late arrival after the user navigated away
        // or switched channels — only apply when the prompt is still
        // on the same channel.
        if *pc != channel {
            return Task::none();
        }
        match result {
            Ok(Some(release)) => {
                *available = Some(release.version.to_string());
                *error = None;
            }
            Ok(None) => {
                *error = Some("No game release published for this channel yet.".to_string());
            }
            Err(e) => {
                tracing::warn!(error = %e, ?channel, "latest_release fetch failed");
                *error = Some(format!("Could not reach GitHub Releases: {e}"));
            }
        }
    }
    Task::none()
}

pub(crate) fn pick_install_location(_state: &mut AppState) -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .set_title("Choose game install location")
                .pick_folder()
                .await
                .map(|fh| fh.path().to_path_buf())
        },
        Message::InstallLocationPicked,
    )
}

pub(crate) fn install_location_picked(
    state: &mut AppState,
    picked: Option<std::path::PathBuf>,
) -> Task<Message> {
    if let Some(path) = picked {
        if let CenterView::InstallPrompt {
            channel,
            install_root,
            error,
            ..
        } = &mut state.center_view
        {
            // Reject a location that would nest the game inside the launcher's
            // own folder. Clear install_root so the rejection is unambiguous —
            // Confirm stays gated and a previously-selected path (e.g. the prior
            // install dir seeded by the update flow) can't be confirmed while
            // the error is showing — and surface the blocking message in the
            // prompt's existing error slot.
            if crate::paths::install_location_collides(&path, channel.dir_name()) {
                tracing::info!(
                    ?channel,
                    path = %path.display(),
                    "rejected install location — collides with launcher dir"
                );
                *install_root = None;
                *error = Some(INVALID_INSTALL_LOCATION_MSG.to_string());
            } else {
                *install_root = Some(path);
                *error = None;
            }
        }
    }
    Task::none()
}

pub(crate) fn install_confirmed(state: &mut AppState) -> Task<Message> {
    // Snapshot the prompt state — we need owned values for the async
    // task. If any piece is missing the Confirm button shouldn't be
    // pressable; logged + ignored as defence-in-depth.
    let (channel, install_root, version) = if let CenterView::InstallPrompt {
        channel,
        install_root: Some(root),
        available: Some(v),
        ..
    } = &state.center_view
    {
        (*channel, root.clone(), v.clone())
    } else {
        tracing::warn!("InstallConfirmed with incomplete prompt state");
        return Task::none();
    };
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("InstallConfirmed for Dev without dev_flag — refusing");
        return Task::none();
    }
    // Refuse the install if /register never produced creds for this
    // channel — InstallComplete's identity-update step would silently
    // no-op (channels.get_mut returns None), leaving install metadata
    // unpersisted and orphaning the on-disk files.
    if !state.identity.channels.contains_key(&channel) {
        tracing::warn!(
            ?channel,
            "InstallConfirmed for {channel} with missing credentials — refusing"
        );
        return Task::none();
    }
    // Defence-in-depth: the picker rejects a colliding location up front, but
    // an update reuses the prior install_root from identity.json (which could
    // be hand-edited). Re-check before any destructive work so the game is
    // never installed inside the launcher's own folder.
    if crate::paths::install_location_collides(&install_root, channel.dir_name()) {
        tracing::warn!(
            ?channel,
            root = %install_root.display(),
            "refusing install — location collides with launcher dir"
        );
        if let CenterView::InstallPrompt { error, .. } = &mut state.center_view {
            *error = Some(INVALID_INSTALL_LOCATION_MSG.to_string());
        }
        return Task::none();
    }
    if state.install_in_progress.is_some() {
        return Task::none();
    }
    // Parse the expected version up front so the staleness check
    // inside the async task compares Version-to-Version (handles
    // canonical-form differences like `1.2.3-dev.1` vs an equivalent
    // non-canonical string) instead of doing string equality. A
    // parse failure here is an upstream contract bug — log loudly
    // but proceed; the downstream installer is the safety net.
    let expected_version = match semver::Version::parse(&version) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::error!(
                error = %e,
                version,
                "expected version from install prompt is not valid semver"
            );
            None
        }
    };
    state.install_in_progress = Some(channel);
    state.download_progress = None;
    // Resolve the release at install time: re-fetch latest, guarding against it
    // vanishing or drifting version between the check and now. Then hand off to
    // the shared streamed-download helper (also used by Repair).
    let resolve = async move {
        let fresh = crate::updater::branches::latest_release(channel).await?;
        let Some(release) = fresh else {
            return Err("release disappeared from GitHub between check and install".to_string());
        };
        if let Some(expected) = expected_version.as_ref() {
            if &release.version != expected {
                tracing::warn!(
                    expected = %expected,
                    actual = %release.version,
                    "release version changed between check and install"
                );
            }
        }
        Ok(release)
    };
    stream_install(channel, install_root, resolve, |channel, result| {
        Message::InstallComplete { channel, result }
    })
}

/// Spawn the streamed download+extract of a resolved `GameRelease` into
/// `install_root`, returning the Iced `Task::stream` that emits
/// `DownloadProgress` events and finally `make_complete(channel, result)`.
/// Centralises the mpsc ⇆ `Task::stream` wiring shared by fresh install/update
/// (`install_confirmed`) and Repair (`repair_confirmed`). `resolve_release`
/// produces the release to install (latest for install, fetch-by-tag for
/// repair); its `Err` short-circuits straight to the completion message.
///
/// Bounded channel: Progress has latest-state semantics so we `try_send` and
/// drop on full (the next chunk corrects the fraction); Complete is one-shot
/// and uses the awaited send so it is never dropped. Capacity 32 ≈ half a
/// second of progress at a 64ms Iced frame on a multi-MB/s download.
fn stream_install<Fut>(
    channel: Channel,
    install_root: std::path::PathBuf,
    resolve_release: Fut,
    make_complete: fn(Channel, Result<crate::updater::branches::InstallResult, String>) -> Message,
) -> Task<Message>
where
    Fut: std::future::Future<Output = Result<crate::updater::branches::GameRelease, String>>
        + Send
        + 'static,
{
    const INSTALL_STREAM_CAPACITY: usize = 32;
    let (tx, rx) = tokio::sync::mpsc::channel::<InstallStreamEvent>(INSTALL_STREAM_CAPACITY);
    let tx_progress = tx.clone();
    tokio::spawn(async move {
        let result: Result<crate::updater::branches::InstallResult, String> = async {
            let release = resolve_release.await?;
            crate::updater::branches::download_and_install(
                channel,
                release,
                install_root,
                move |progress| {
                    let _ = tx_progress.try_send(InstallStreamEvent::Progress(progress));
                },
            )
            .await
        }
        .await;
        let _ = tx.send(InstallStreamEvent::Complete(result)).await;
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(move |ev| match ev {
        InstallStreamEvent::Progress(progress) => Message::DownloadProgress { channel, progress },
        InstallStreamEvent::Complete(result) => make_complete(channel, result),
    });
    Task::stream(stream)
}

pub(crate) fn download_progress(
    state: &mut AppState,
    channel: Channel,
    progress: crate::updater::branches::InstallProgress,
) -> Task<Message> {
    // Late events for a stale channel can arrive if the user
    // cancels and starts another install before the previous
    // stream drains. Discard those.
    if state.install_in_progress != Some(channel) {
        return Task::none();
    }
    state.download_progress = Some(progress);
    Task::none()
}

pub(crate) fn install_complete(
    state: &mut AppState,
    channel: Channel,
    result: Result<crate::updater::branches::InstallResult, String>,
) -> Task<Message> {
    state.install_in_progress = None;
    state.download_progress = None;
    match result {
        Ok(info) => {
            apply_install_success(state, channel, &info);
            tracing::info!(
                ?channel,
                version = %info.version,
                exe = %info.executable,
                install_dir = %info.install_dir.display(),
                "game install complete"
            );
            state.center_view = CenterView::Default;
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "game install failed");
            if let CenterView::InstallPrompt {
                channel: pc,
                error: prompt_error,
                ..
            } = &mut state.center_view
            {
                if *pc == channel {
                    *prompt_error = Some(format!("Install failed: {e}"));
                }
            }
        }
    }
    Task::none()
}

/// Persist a successful install/repair into identity.json and refresh derived
/// UI state (the top-bar update banner, plus the now-stale firewall and
/// update-check verdicts for this channel). Shared by `install_complete` and
/// `repair_complete` so the post-install bookkeeping lives in one place.
fn apply_install_success(
    state: &mut AppState,
    channel: Channel,
    info: &crate::updater::branches::InstallResult,
) {
    if let Some(creds) = state.identity.channels.get_mut(&channel) {
        creds.install_location = Some(info.install_dir.clone());
        creds.installed_version = Some(info.version.clone());
    }
    if let Err(e) = identity::save(&state.identity) {
        tracing::warn!(error = %e, ?channel, "failed to persist identity after install");
    }
    // installed_version just changed — drop this channel from the banner.
    recompute_branch_updates_available(state);
    // A (re)install can change the resolved exe path → stale firewall result.
    state.firewall_status.remove(&channel);
    // Version changed → a stale "Update available" verdict would mislead.
    state.channel_update_status.remove(&channel);
}

/// User confirmed Repair — reinstall the *installed* version (fetch-by-tag) via
/// the shared streamed-download pipeline. Reuses the transactional installer, so
/// a failed repair leaves the existing install untouched. Routes completion to
/// `RepairComplete`.
pub(crate) fn repair_confirmed(state: &mut AppState) -> Task<Message> {
    let (channel, version) =
        if let CenterView::RepairConfirm { channel, version, .. } = &state.center_view {
            (*channel, version.clone())
        } else {
            tracing::warn!("RepairConfirmed without an active RepairConfirm view");
            return Task::none();
        };
    if channel == Channel::Dev && !state.dev_flag {
        tracing::warn!("RepairConfirmed for Dev without dev_flag — refusing");
        return Task::none();
    }
    // Re-check the busy gates (defence-in-depth against a press that raced a
    // state change after the prompt opened).
    if state.install_in_progress.is_some()
        || state.uninstall_in_progress.is_some()
        || state.game_running
    {
        return Task::none();
    }
    // Repair reinstalls into the same place — derive the install_root (parent of
    // the recorded install dir, since download_and_install rejoins the channel).
    let install_root = match state
        .identity
        .channels
        .get(&channel)
        .and_then(|c| c.install_location.clone())
    {
        Some(dir) => match dir.parent() {
            Some(p) => p.to_path_buf(),
            None => {
                set_repair_error(state, channel, "Could not resolve the install location.");
                return Task::none();
            }
        },
        None => {
            tracing::warn!(?channel, "RepairConfirmed with no install on record");
            return Task::none();
        }
    };
    let target = match semver::Version::parse(&version) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, version, "installed version is not valid semver");
            set_repair_error(state, channel, &format!("Recorded version '{version}' is not valid."));
            return Task::none();
        }
    };
    state.install_in_progress = Some(channel);
    state.download_progress = None;
    tracing::info!(?channel, %version, "starting repair reinstall");
    let resolve = async move {
        match crate::updater::branches::release_for_version(channel, &target).await? {
            Some(release) => Ok(release),
            None => Err(format!(
                "v{target} is no longer available for this channel on GitHub — \
                 use Update to install the latest version instead."
            )),
        }
    };
    stream_install(channel, install_root, resolve, |channel, result| {
        Message::RepairComplete { channel, result }
    })
}

/// Set the inline error on the open RepairConfirm prompt (no-op if the prompt
/// moved on). Keeps the prompt open so the user can read the failure and retry.
fn set_repair_error(state: &mut AppState, channel: Channel, msg: &str) {
    if let CenterView::RepairConfirm { channel: pc, error, .. } = &mut state.center_view {
        if *pc == channel {
            *error = Some(msg.to_string());
        }
    }
}

/// Repair reinstall finished. On success: persist, return to channel management,
/// and auto re-verify so the user sees the install is healed. On failure: keep
/// the prompt open with the error (the prior install is untouched).
pub(crate) fn repair_complete(
    state: &mut AppState,
    channel: Channel,
    result: Result<crate::updater::branches::InstallResult, String>,
) -> Task<Message> {
    state.install_in_progress = None;
    state.download_progress = None;
    match result {
        Ok(info) => {
            apply_install_success(state, channel, &info);
            tracing::info!(?channel, version = %info.version, "repair complete — re-verifying");
            state.center_view = CenterView::Settings {
                tab: crate::app::SettingsTab::ChannelManagement,
            };
            super::maintenance::verify_channel(state, channel)
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "repair failed");
            set_repair_error(state, channel, &format!("Repair failed: {e}"));
            Task::none()
        }
    }
}

pub(crate) fn latest_release_fetched(
    state: &mut AppState,
    channel: Channel,
    result: Result<Option<crate::updater::branches::GameRelease>, String>,
) -> Task<Message> {
    // Dev gating defence-in-depth: a late-arriving Dev fetch when
    // the user is no longer dev-flagged must not poison the cache.
    // (latest_release_tasks only spawns for visible channels, but a
    // request issued before the flag was revoked could still
    // resolve after.)
    if channel == Channel::Dev && !state.dev_flag {
        tracing::debug!("dropping late LatestReleaseFetched(Dev) — dev_flag is false");
        return Task::none();
    }
    match result {
        Ok(Some(release)) => {
            state.available_versions.insert(channel, release.version);
        }
        Ok(None) => {
            state.available_versions.remove(&channel);
            tracing::info!(?channel, "no game release published for this channel yet");
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "latest_release fetch failed");
            // Leave any prior cached version in place — the user
            // still sees something while GitHub recovers.
        }
    }
    recompute_branch_updates_available(state);
    Task::none()
}

/// User pressed the left-rail "Check for Updates" button for `channel`.
/// Re-runs the same GitHub `latest_release` query the boot fan-out uses, so a
/// release published since launch is picked up. Marks the channel `Checking`
/// (disabling the button) and hands the result back via `ChannelUpdateCheckDone`.
pub(crate) fn check_channel_update_pressed(
    state: &mut AppState,
    channel: Channel,
) -> Task<Message> {
    // Dev gating defence-in-depth — the row is only rendered for visible
    // channels (Dev hidden unless dev_flag), but never reach the GitHub API
    // for Dev without the server-assigned flag regardless of UI state.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::debug!("ignoring Dev update check — dev_flag is false");
        return Task::none();
    }
    // Block a double-press while a check for this channel is already running.
    if matches!(
        state.channel_update_status.get(&channel),
        Some(crate::app::ChannelUpdateStatus::Checking)
    ) {
        return Task::none();
    }
    // Rate-limit gate: if the back-off window is open, show the resume time
    // immediately instead of spending (and failing) a GitHub request.
    if let crate::ratelimit::Gate::Blocked { resume_at } = crate::ratelimit::gate() {
        state.channel_update_status.insert(
            channel,
            crate::app::ChannelUpdateStatus::RateLimited {
                resume_at: crate::ratelimit::format_resume(resume_at),
            },
        );
        return Task::none();
    }
    state
        .channel_update_status
        .insert(channel, crate::app::ChannelUpdateStatus::Checking);
    Task::perform(
        crate::updater::branches::latest_release(channel),
        move |result| Message::ChannelUpdateCheckDone { channel, result },
    )
}

/// A manual `check_channel_update_pressed` fetch landed. Refreshes
/// `available_versions[channel]` (the single source the bottom-bar button reads —
/// its logic is untouched) and records the user-facing verdict in
/// `channel_update_status` for the left-rail status box.
pub(crate) fn channel_update_check_done(
    state: &mut AppState,
    channel: Channel,
    result: Result<Option<crate::updater::branches::GameRelease>, String>,
) -> Task<Message> {
    // Same late-arrival Dev guard as latest_release_fetched: a check issued
    // before the flag was revoked must not poison the cache or status box.
    if channel == Channel::Dev && !state.dev_flag {
        tracing::debug!("dropping late ChannelUpdateCheckDone(Dev) — dev_flag is false");
        state.channel_update_status.remove(&channel);
        return Task::none();
    }
    let installed = state
        .identity
        .channels
        .get(&channel)
        .and_then(|c| c.parsed_installed_version());
    let status = match result {
        Ok(Some(release)) => {
            let status = crate::app::ChannelUpdateStatus::from_check(
                installed.as_ref(),
                Some(&release.version),
            );
            state.available_versions.insert(channel, release.version);
            status
        }
        Ok(None) => {
            // No release published for this channel — nothing newer than disk.
            state.available_versions.remove(&channel);
            tracing::info!(?channel, "no game release published for this channel yet");
            crate::app::ChannelUpdateStatus::from_check(installed.as_ref(), None)
        }
        Err(e) => {
            tracing::warn!(error = %e, ?channel, "manual update check fetch failed");
            // Leave any prior cached available_versions in place — the bottom
            // button keeps showing what it knew before this failed re-check.
            crate::app::ChannelUpdateStatus::Failed
        }
    };
    state.channel_update_status.insert(channel, status);
    recompute_branch_updates_available(state);
    Task::none()
}
