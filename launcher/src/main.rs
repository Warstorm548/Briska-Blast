//! Launcher entry point. Synchronous main — Iced owns the async runtime
//! via its `tokio` feature; we do not need `#[tokio::main]`.

// Hide the Windows console window for release builds. Debug builds keep
// it so `cargo run -p launcher` still shows tracing output. Non-Windows
// targets ignore this attribute entirely.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod channel;
mod game_launch;
mod identity;
mod paths;
mod server_api;
mod ui;
mod updater;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> iced::Result {
    init_tracing();
    updater::cleanup_stale_update_artifacts();
    iced::application(app::boot, app::update, app::view)
        .title(app::title)
        .theme(app::theme)
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
