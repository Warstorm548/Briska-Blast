//! Launcher entry point. Synchronous main — Iced owns the async runtime
//! via its `tokio` feature; we do not need `#[tokio::main]`.

// Hide the Windows console window for release builds. Debug builds keep
// it so `cargo run -p launcher` still shows tracing output. Non-Windows
// targets ignore this attribute entirely.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod channel;
mod firewall;
mod game_launch;
mod identity;
mod paths;
mod ratelimit;
mod rendezvous;
mod server_api;
mod ui;
mod updater;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> iced::Result {
    init_tracing();

    // Single-instance gate (socket rendezvous): a second launcher detects the
    // first and exits cleanly. Run before cleanup_stale_update_artifacts so a
    // duplicate never disturbs the live instance's in-flight update files. The
    // guard holds the claim — and removes its discovery file on a clean exit —
    // for as long as it is in scope, i.e. across the whole `.run()` below.
    let _instance_guard = match rendezvous::acquire_launcher() {
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
