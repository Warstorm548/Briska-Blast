//! Launcher self-update flow: the GitHub-Releases update check and the
//! rename-trick binary swap.

use crate::app::{AppState, Message};
use crate::updater::release_cache::Freshness;
use crate::updater::{self, UpdateCheckOutcome};
use futures_util::StreamExt;
use iced::Task;

pub(crate) fn check_for_updates_pressed(state: &mut AppState) -> Task<Message> {
    if !state.update_check_in_flight && !state.self_update_in_flight {
        // Rate-limit gate: if the back-off window is open, surface the resume time
        // instead of spending (and failing) a GitHub request.
        if let crate::ratelimit::Gate::Blocked { resume_at } = crate::ratelimit::gate() {
            state.last_self_update_error = Some(format!(
                "GitHub rate limit reached \u{2014} checks resume at {}.",
                crate::ratelimit::format_resume(resume_at)
            ));
            return Task::none();
        }
        state.update_check_in_flight = true;
        state.last_self_update_error = None;
        // Revalidate: the user explicitly asked, so this must reach GitHub even
        // with a warm snapshot. It spends a request either way; an unchanged repo
        // just answers `304` without re-sending the body.
        return Task::perform(
            updater::check_for_update(Freshness::Revalidate),
            Message::LauncherUpdateCheckDone,
        );
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
            // The release body is no longer displayed — the Launcher Update
            // view now renders the changelog entries between the running and
            // target versions, which is real content. Launcher releases publish
            // an empty body anyway, so the old preview never showed anything.
            let _ = notes;
            // Re-seed the accordion onto the newly known target version.
            state
                .changelog_open
                .remove(&crate::changelog::Kind::Launcher);
        }
        Ok(UpdateCheckOutcome::UpToDate) => {
            state.launcher_update_available = false;
            state.launcher_available_version.clear();
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
    } else if state.install_in_progress.is_some() {
        // The two jobs share `active_update`, so a self-update starting here
        // would overwrite the install's plan and leave its phases resolving
        // against the wrong step list. Worse, a successful self-update ends in
        // `process::exit(0)`, which would kill the install mid staging-swap.
        tracing::warn!("refusing self-update: a game install is in flight");
        state.last_self_update_error =
            Some("Cannot update the launcher while a game install is running.".into());
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

        // A self-update is a two-step job: fetch the asset, then swap the
        // binary. There is no `files.json` for a single executable, so there is
        // no verify step — the download's magic-byte check is the integrity
        // gate. The step count comes from the plan, not from a constant, which
        // is why the same renderer shows "1/2" here and "1/3" for a game.
        let plan = crate::updater::plan::UpdatePlan::launcher();
        state.active_update = Some(crate::app::ActiveUpdate::starting(plan));

        return stream_self_update(version);
    }
    Task::none()
}

/// Internal pipe between the self-update task and the Iced stream adapter.
/// Mirrors the installer's `InstallStreamEvent` — the two jobs report through
/// the same event type so one handler folds both into the progress bar.
enum SelfUpdateStreamEvent {
    Progress(crate::updater::branches::InstallProgress),
    Complete(Result<(), String>),
}

/// Run the self-update as a progress-emitting stream.
///
/// Bounded channel with the same semantics as the installer's: progress has
/// latest-state meaning so it is `try_send` and dropped when full (the next
/// chunk corrects the fraction), while the one-shot completion is awaited so it
/// can never be lost.
fn stream_self_update(version: String) -> Task<Message> {
    const SELF_UPDATE_STREAM_CAPACITY: usize = 32;
    let (tx, rx) =
        tokio::sync::mpsc::channel::<SelfUpdateStreamEvent>(SELF_UPDATE_STREAM_CAPACITY);
    let tx_progress = tx.clone();
    tokio::spawn(async move {
        let result = updater::run_self_update(version, move |progress| {
            let _ = tx_progress.try_send(SelfUpdateStreamEvent::Progress(progress));
        })
        .await;
        let _ = tx.send(SelfUpdateStreamEvent::Complete(result)).await;
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|ev| match ev {
        SelfUpdateStreamEvent::Progress(progress) => Message::SelfUpdateProgress(progress),
        SelfUpdateStreamEvent::Complete(result) => Message::SelfUpdateDone(result),
    });
    Task::stream(stream)
}

/// Fold a self-update progress event into the bar, via the same helper the game
/// installer uses.
pub(crate) fn self_update_progress(
    state: &mut AppState,
    progress: crate::updater::branches::InstallProgress,
) -> Task<Message> {
    if !state.self_update_in_flight {
        return Task::none();
    }
    super::install::apply_progress(state, progress);
    Task::none()
}

pub(crate) fn self_update_done(state: &mut AppState, result: Result<(), String>) -> Task<Message> {
    state.self_update_in_flight = false;
    match result {
        Ok(()) => {
            // The binary on disk has been swapped, so this process is now
            // running code that no longer exists on disk and must not carry on.
            // Start the replacement *before* exiting: previously this just
            // exited, leaving the user with no launcher and nothing telling
            // them to start one.
            tracing::info!("self-update succeeded — starting replacement");
            if let Err(e) = updater::spawn_replacement() {
                // Nothing left to fall back on: the swap already happened, so
                // staying open would keep running the old code. Log loudly and
                // still exit — the user restarts by hand, which is exactly the
                // behaviour before this change.
                tracing::error!(error = %e, "could not start the replacement launcher");
            }
            std::process::exit(0);
        }
        Err(e) => {
            tracing::error!(error = %e, "self-update failed");
            state.last_self_update_error = Some(format!("Update failed: {e}"));
            // Only clear the shared bar if it is still ours. The start guards
            // make the two jobs mutually exclusive, so this is defence in depth.
            if state.install_in_progress.is_none() {
                state.active_update = None;
            }
        }
    }
    Task::none()
}
