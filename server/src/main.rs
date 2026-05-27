mod admin;
mod api;
mod config;
mod error;
mod gamemode;
mod middleware;
mod signaling;
mod state;
mod testharness;
mod update;

use axum::{
    middleware as axum_middleware,
    routing::{delete, get, post},
    Router,
};
use deadpool_redis::{redis::AsyncCommands, Config as RedisConfig, Runtime};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let cfg = config::Config::from_env();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let redis_cfg = RedisConfig::from_url(&cfg.redis_url);
    let redis_pool = redis_cfg
        .create_pool(Some(Runtime::Tokio1))
        .expect("failed to create Redis pool");

    seed_defaults(&redis_pool, &cfg).await;

    let game_port = cfg.game_port;
    let admin_port = cfg.admin_port;

    // bootstrap update task before AppState so the sender is ready
    let (update_tx_inner, update_rx) = tokio::sync::mpsc::channel::<update::UpdateCommand>(32);
    let update_tx = std::sync::Arc::new(update_tx_inner);

    let state = state::AppState::new(redis_pool, cfg, update_tx.clone());

    // spawn update background task
    tokio::spawn(update::task::run(state.clone(), update_rx));

    // Game router — no /admin routes
    let versioned = Router::new()
        .route("/host", post(api::host::host))
        .route("/join", post(api::join::join))
        .route("/session/:code/start", post(api::start::start_session))
        .route("/session/:code/host", post(api::session::transfer_host))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::version::check_version,
        ));

    let mut game_router = Router::new()
        .route("/register", post(api::register::register))
        .route("/me/username", post(api::me::update_username))
        .route("/session/:code", get(api::session::get_session))
        .route("/session/:code", delete(api::session::close_session))
        .route("/ws/session/:code", get(signaling::ws::ws_handler))
        .merge(versioned);

    if std::env::var("ENABLE_TEST_HARNESS")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        tracing::warn!("ENABLE_TEST_HARNESS=true — /test/webrtc route is exposed");
        game_router = game_router.route("/test/webrtc", get(testharness::page));
    }

    let game_router = game_router
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    // Admin router — no game routes
    let admin_router = Router::new()
        .route("/admin", get(admin::auth::login_page))
        .route("/admin/login", post(admin::auth::login))
        .route("/admin/logout", post(admin::auth::logout))
        .route("/admin/dashboard", get(admin::dashboard::dashboard))
        .route("/admin/update/launcher-version", post(admin::dashboard::update_launcher_version))
        .route("/admin/update/game-version", post(admin::dashboard::update_game_version))
        .route("/admin/update/password", post(admin::dashboard::update_password))
        .route("/admin/update/check", post(admin::dashboard::check_for_update))
        .route("/admin/update/apply-now", post(admin::dashboard::apply_update_now))
        .route("/admin/update/schedule", post(admin::dashboard::schedule_update))
        .route("/admin/update/cancel", post(admin::dashboard::cancel_update))
        .route("/admin/update/settings", post(admin::dashboard::save_update_settings))
        .route("/admin/update/rollback", post(admin::dashboard::rollback_update))
        .route("/admin/users", get(admin::users::users_page))
        .route("/admin/users/dev-flag", post(admin::users::save_dev_flags))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Bind — fail fast with an actionable error message
    let game_addr: SocketAddr = format!("0.0.0.0:{game_port}").parse().unwrap();
    let game_listener = tokio::net::TcpListener::bind(game_addr).await
        .unwrap_or_else(|e| {
            tracing::error!(
                "Failed to bind game listener to {game_addr}: {e}.\n      \
                 HINT: set GAME_PORT in .env to a different value and restart."
            );
            std::process::exit(1);
        });
    tracing::info!("game listener bound to 0.0.0.0:{game_port}");

    let admin_addr: SocketAddr = format!("0.0.0.0:{admin_port}").parse().unwrap();
    let admin_listener = tokio::net::TcpListener::bind(admin_addr).await
        .unwrap_or_else(|e| {
            tracing::error!(
                "Failed to bind admin listener to {admin_addr}: {e}.\n      \
                 HINT: set ADMIN_PORT in .env to a different value and restart."
            );
            std::process::exit(1);
        });
    tracing::info!("admin listener bound to 0.0.0.0:{admin_port}");

    // Graceful shutdown — one watcher task broadcasts () to both listeners.
    // Two independent signal futures can't both receive SIGTERM (single-consumer),
    // so we use a broadcast channel so both servers drain cleanly.
    let (shutdown_tx, _sentinel) = tokio::sync::broadcast::channel::<()>(1);
    let mut rx_game = shutdown_tx.subscribe();
    let mut rx_admin = shutdown_tx.subscribe();
    let watcher_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => { tracing::info!("received Ctrl+C — shutting down"); }
                _ = sigterm.recv() => { tracing::info!("received SIGTERM — shutting down"); }
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            tracing::info!("received Ctrl+C — shutting down");
        }
        watcher_tx.send(()).ok();
    });

    let game_serve = axum::serve(
        game_listener,
        game_router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { rx_game.recv().await.ok(); });

    let admin_serve = axum::serve(
        admin_listener,
        admin_router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move { rx_admin.recv().await.ok(); });

    tokio::try_join!(game_serve, admin_serve).unwrap();
}

async fn seed_defaults(pool: &deadpool_redis::Pool, cfg: &config::Config) {
    let Ok(mut conn) = pool.get().await else { return };

    let _: () = conn
        .set_nx("min_launcher_version", &cfg.min_launcher_version)
        .await
        .unwrap_or(());
    let _: () = conn
        .set_nx("min_game_version", &cfg.min_game_version)
        .await
        .unwrap_or(());

    let exists: bool = conn
        .exists("admin:password_hash")
        .await
        .unwrap_or(false);
    if !exists {
        let password = cfg.admin_password.clone();
        if let Ok(Ok(hash)) =
            tokio::task::spawn_blocking(move || bcrypt::hash(&password, bcrypt::DEFAULT_COST))
                .await
        {
            let _: () = conn.set("admin:password_hash", hash).await.unwrap_or(());
        }
    }
}
