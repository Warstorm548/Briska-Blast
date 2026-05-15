use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use deadpool_redis::redis::AsyncCommands;
use semver::Version;

use crate::{error::AppError, state::AppState};

async fn check_one(
    conn: &mut deadpool_redis::Connection,
    header_val: &str,
    redis_key: &str,
    fallback: &str,
    component: &str,
) -> std::result::Result<(), AppError> {
    let minimum: String = conn
        .get(redis_key)
        .await
        .unwrap_or_else(|_| fallback.to_string());

    let current_ver = Version::parse(header_val).unwrap_or(Version::new(0, 0, 0));
    let minimum_ver = Version::parse(&minimum).unwrap_or(Version::new(0, 1, 0));

    if current_ver < minimum_ver {
        return Err(AppError::UpgradeRequired {
            component: component.to_string(),
            minimum,
            current: header_val.to_string(),
        });
    }
    Ok(())
}

pub async fn check_version(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, AppError> {
    let launcher_ver = request
        .headers()
        .get("X-Launcher-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0.0.0")
        .to_string();

    let game_ver = request
        .headers()
        .get("X-Game-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0.0.0")
        .to_string();

    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    check_one(
        &mut conn,
        &launcher_ver,
        "min_launcher_version",
        &state.config.min_launcher_version,
        "launcher",
    )
    .await?;

    check_one(
        &mut conn,
        &game_ver,
        "min_game_version",
        &state.config.min_game_version,
        "game",
    )
    .await?;

    Ok(next.run(request).await)
}
