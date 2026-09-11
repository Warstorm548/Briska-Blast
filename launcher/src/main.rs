//! Launcher entry point. Synchronous main — Iced owns the async runtime
//! via its `tokio` feature; we do not need `#[tokio::main]`.

// Hide the Windows console window for release builds. Debug builds keep
// it so `cargo run -p launcher` still shows tracing output. Non-Windows
// targets ignore this attribute entirely.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod channel;
/// In-app game + launcher changelogs: bundled baseline, disk cache, and a
/// conditional refresh from GitHub's raw file CDN (not the rate-limited API).
mod changelog;
mod firewall;
mod game_launch;
mod identity;
mod paths;
mod ratelimit;
mod rendezvous;
mod server_api;
mod ui;
mod updater;

use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// How long a post-update relaunch waits for the outgoing launcher to release
/// the single-instance slot. Generous: the parent exits within milliseconds,
/// so reaching this means a genuinely separate instance is running and this
/// process should stand down.
const RELAUNCH_SLOT_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> iced::Result {
    init_tracing();

    // A launcher started by an outgoing one after a self-update waits for the
    // slot instead of conceding on the first live instance: for a few
    // milliseconds both processes exist, and quitting there would leave the
    // user with no launcher at all. See `updater::relaunch`.
    let after_update = std::env::args().any(|a| a == updater::AFTER_UPDATE_ARG);

    // Single-instance gate (socket rendezvous): a second launcher detects the
    // first and exits cleanly. Run before cleanup_stale_update_artifacts so a
    // duplicate never disturbs the live instance's in-flight update files, and
    // — on the relaunch path — so the Windows `.__relocated__.exe` left by the
    // swap is only deleted once the process holding it has actually exited.
    // The guard holds the claim — and removes its discovery file on a clean
    // exit — for as long as it is in scope, i.e. across the whole `.run()`.
    let acquired = if after_update {
        tracing::info!("started as a post-update relaunch — waiting for the instance slot");
        rendezvous::acquire_launcher_waiting(RELAUNCH_SLOT_TIMEOUT)
    } else {
        rendezvous::acquire_launcher()
    };
    let _instance_guard = match acquired {
        rendezvous::AcquireOutcome::Live(guard) => guard,
        rendezvous::AcquireOutcome::Duplicate => {
            tracing::info!("another launcher instance is already running — exiting");
            return Ok(());
        }
    };

    updater::cleanup_stale_update_artifacts();
    iced::application(app::boot, app::update, app::view)
        .title(app::title)
        .theme(app::theme)
        .subscription(app::subscription)
        .run()
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "launcher=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
