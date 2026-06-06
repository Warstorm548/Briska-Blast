use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::{self, AsyncCommands};

use super::{require_session, templates};
use crate::state::AppState;

/// GET /admin/stats — dedicated server-statistics page. Reuses the same
/// counters surfaced on the dashboard (live session count + total players);
/// further metrics are scaffolded as placeholders in the template for now.
pub async fn stats(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => return Html(format!("Redis error: {e}")).into_response(),
    };

    let player_count: u64 = conn.get("player:counter").await.unwrap_or(0u64);

    // Mirrors the dashboard's `KEYS session:*` scan. Player/session sets are
    // small in the pre-stable window; revisit with SCAN once they grow.
    let session_keys: Vec<String> = redis::cmd("KEYS")
        .arg("session:*")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();
    let session_count = session_keys.len();

    Html(templates::stats_page(session_count, player_count)).into_response()
}
