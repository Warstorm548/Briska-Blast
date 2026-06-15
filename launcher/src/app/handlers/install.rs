//! Per-channel game install / update pipeline: the install prompt, the
//! folder picker, the streamed download+extract, and the boot-time
//! `latest_release` fan-out that feeds the bottom-left button + update banner.

use crate::app::{recompute_branch_updates_available, AppState, CenterView, Message};
use crate::channel::Channel;
use crate::identity;
use futures_util::StreamExt;
use iced::Task;

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
        if let CenterView::InstallPrompt { install_root, .. } = &mut state.center_view {
            *install_root = Some(path);
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
    // Drive the install with two channels: a tokio::spawn task that
    // owns the download (and produces Progress events into an mpsc
    // sender as it streams), and a Task::stream that consumes the
    // matching receiver and emits Messages into the Iced runtime.
    // The same Sender is cloned for the closure callback; the
    // outer clone fires the final Complete event once the .await
    // on download_and_install resolves.
    //
    // Bounded channel — Progress has latest-state semantics so we
    // try_send and drop on full (the next chunk's event arrives
    // shortly anyway). Complete is one-shot and must not be
    // dropped, so it uses the awaited send. Capacity 32 covers
    // ~half a second of progress events at a 64ms Iced frame and
    // a multi-MB/s download.
    const INSTALL_STREAM_CAPACITY: usize = 32;
    let (tx, rx) = tokio::sync::mpsc::channel::<InstallStreamEvent>(INSTALL_STREAM_CAPACITY);
    let tx_progress = tx.clone();
    tokio::spawn(async move {
        let result: Result<crate::updater::branches::InstallResult, String> = async {
            let fresh = crate::updater::branches::latest_release(channel).await?;
            let Some(release) = fresh else {
                return Err(
                    "release disappeared from GitHub between check and install".to_string(),
                );
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
            crate::updater::branches::download_and_install(
                channel,
                release,
                install_root,
                move |progress| {
                    // Drop on full / receiver-dropped — progress is
                    // safe to skip (the next event corrects the
                    // displayed fraction); blocking the callback
                    // would stall the actual download loop.
                    let _ = tx_progress.try_send(InstallStreamEvent::Progress(progress));
                },
            )
            .await
        }
        .await;
        // Completion must not be dropped — `send().await` waits
        // for capacity. If the receiver has been dropped we don't
        // care (no UI to update); the `let _ =` absorbs that.
        let _ = tx.send(InstallStreamEvent::Complete(result)).await;
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(move |ev| match ev {
        InstallStreamEvent::Progress(progress) => Message::DownloadProgress { channel, progress },
        InstallStreamEvent::Complete(result) => Message::InstallComplete { channel, result },
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
            if let Some(creds) = state.identity.channels.get_mut(&channel) {
                creds.install_location = Some(info.install_dir.clone());
                creds.installed_version = Some(info.version.clone());
            }
            if let Err(e) = identity::save(&state.identity) {
                tracing::warn!(
                    error = %e,
                    ?channel,
                    "failed to persist identity after install"
                );
            }
            tracing::info!(
                ?channel,
                version = %info.version,
                exe = %info.executable,
                install_dir = %info.install_dir.display(),
                "game install complete"
            );
            // installed_version just changed — refresh the derived
            // top-bar banner so this channel falls out of the
            // "Updates available" list.
            recompute_branch_updates_available(state);
            // A (re)install can change the resolved game exe path, so
            // any firewall result cached against the old install is no
            // longer trustworthy — clear it and let the user re-check.
            state.firewall_status.remove(&channel);
            // Same for the manual update-check verdict: the version just
            // changed, so a stale "Update available" would be misleading.
            state.channel_update_status.remove(&channel);
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

/// User pressed "Check for Updates" for `channel` (Settings → Channel Updates).
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
/// `channel_update_status` for the Settings status box.
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
