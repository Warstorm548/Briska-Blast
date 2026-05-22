//! Launcher entry point. Synchronous main — Iced owns the async runtime
//! via its `tokio` feature; we do not need `#[tokio::main]`.

mod app;
mod channel;
mod identity;
mod mock;
mod ui;
mod updater;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> iced::Result {
    init_tracing();
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
