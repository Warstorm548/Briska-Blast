mod admin;
mod api;
mod config;
mod error;
mod middleware;
mod state;

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

    let bind_addr_str = resolve_bind_addr(&redis_pool, &cfg.bind_addr).await;
    let bind_addr: SocketAddr = bind_addr_str.parse().expect("invalid bind address");

    let state = state::AppState::new(redis_pool, cfg, bind_addr_str);

    let versioned = Router::new()
        .route("/host", post(api::host::host))
        .route("/join", post(api::join::join))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::version::check_version,
        ));

    let app = Router::new()
        .route("/register", post(api::register::register))
        .route("/session/:code", get(api::session::get_session))
        .route("/session/:code", delete(api::session::close_session))
        .merge(versioned)
        .route("/admin", get(admin::auth::login_page))
        .route("/admin/login", post(admin::auth::login))
        .route("/admin/logout", post(admin::auth::logout))
        .route("/admin/dashboard", get(admin::dashboard::dashboard))
        .route("/admin/update/launcher-version", post(admin::dashboard::update_launcher_version))
        .route("/admin/update/game-version", post(admin::dashboard::update_game_version))
        .route("/admin/update/bind-addr", post(admin::dashboard::update_bind_addr))
        .route("/admin/update/password", post(admin::dashboard::update_password))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!("listening on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("failed to bind");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
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
    let _: () = conn
        .set_nx("server:bind_addr", &cfg.bind_addr)
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

async fn resolve_bind_addr(pool: &deadpool_redis::Pool, fallback: &str) -> String {
    if let Ok(mut conn) = pool.get().await {
        if let Ok(Some(addr)) = conn.get::<_, Option<String>>("server:bind_addr").await {
            if !addr.is_empty() {
                return addr;
            }
        }
    }
    fallback.to_string()
}
