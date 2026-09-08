use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Unauthorized,
    BadRequest(String),
    Conflict(String),
    TooManyRequests,
    UpgradeRequired { component: String, minimum: String, current: String },
    InvalidPlayerCount { min: u8, max: u8, requested: u8 },
    InvalidWinCondition { min: u8, max: u8, requested: u8 },
    InvalidSpawnSettings { min: u8, max: u8, requested: u8 },
    /// Unlike the three above, loot settings have several independently-bounded
    /// fields plus a cap on their combined total, so this one names the offending
    /// field — "invalid loot settings" alone would not tell a host which slider to
    /// move. `requested` is u16 because the weight total can exceed a u8.
    InvalidLootSettings { field: &'static str, min: u16, max: u16, requested: u16 },
    SessionFull { capacity: u8 },
    SessionNotStartable { reason: &'static str },
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, json!({"error": "not_found"})),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"})),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, json!({"error": msg})),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, json!({"error": msg})),
            AppError::TooManyRequests => {
                (StatusCode::TOO_MANY_REQUESTS, json!({"error": "rate_limit_exceeded"}))
            }
            AppError::UpgradeRequired { component, minimum, current } => (
                StatusCode::from_u16(426).unwrap(),
                json!({
                    "error": format!("{}_update_required", component),
                    "minimum_version": minimum,
                    "current_version": current,
                }),
            ),
            AppError::InvalidPlayerCount { min, max, requested } => (
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid_player_count",
                    "min": min,
                    "max": max,
                    "requested": requested,
                }),
            ),
            AppError::InvalidWinCondition { min, max, requested } => (
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid_win_condition",
                    "min": min,
                    "max": max,
                    "requested": requested,
                }),
            ),
            AppError::InvalidSpawnSettings { min, max, requested } => (
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid_spawn_settings",
                    "min": min,
                    "max": max,
                    "requested": requested,
                }),
            ),
            AppError::InvalidLootSettings { field, min, max, requested } => (
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid_loot_settings",
                    "field": field,
                    "min": min,
                    "max": max,
                    "requested": requested,
                }),
            ),
            AppError::SessionFull { capacity } => (
                StatusCode::CONFLICT,
                json!({"error": "session_full", "capacity": capacity}),
            ),
            AppError::SessionNotStartable { reason } => (
                StatusCode::CONFLICT,
                json!({"error": "session_not_startable", "reason": reason}),
            ),
            AppError::Internal(msg) => {
                tracing::error!("internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": "internal_server_error"}),
                )
            }
        };
        (status, Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(err: AppError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn invalid_player_count_returns_400_with_bounds() {
        let (status, body) =
            body_json(AppError::InvalidPlayerCount { min: 2, max: 4, requested: 5 }).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_player_count");
        assert_eq!(body["min"], 2);
        assert_eq!(body["max"], 4);
        assert_eq!(body["requested"], 5);
    }

    #[tokio::test]
    async fn invalid_win_condition_returns_400_with_bounds() {
        let (status, body) =
            body_json(AppError::InvalidWinCondition { min: 50, max: 200, requested: 49 }).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_win_condition");
        assert_eq!(body["min"], 50);
        assert_eq!(body["max"], 200);
        assert_eq!(body["requested"], 49);
    }

    #[tokio::test]
    async fn invalid_spawn_settings_returns_400_with_bounds() {
        let (status, body) =
            body_json(AppError::InvalidSpawnSettings { min: 5, max: 60, requested: 99 }).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_spawn_settings");
        assert_eq!(body["min"], 5);
        assert_eq!(body["max"], 60);
        assert_eq!(body["requested"], 99);
    }

    #[tokio::test]
    async fn invalid_loot_settings_returns_400_naming_the_field() {
        let (status, body) = body_json(AppError::InvalidLootSettings {
            field: "barrier_weight",
            min: 1,
            max: 100,
            requested: 0,
        })
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_loot_settings");
        assert_eq!(body["field"], "barrier_weight");
        assert_eq!(body["min"], 1);
        assert_eq!(body["max"], 100);
        assert_eq!(body["requested"], 0);
    }

    #[tokio::test]
    async fn invalid_loot_settings_reports_an_oversubscribed_total() {
        let (status, body) = body_json(AppError::InvalidLootSettings {
            field: "weight_total",
            min: 0,
            max: 100,
            requested: 130,
        })
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["field"], "weight_total");
        assert_eq!(body["requested"], 130);
    }

    #[tokio::test]
    async fn session_full_returns_409_with_capacity() {
        let (status, body) = body_json(AppError::SessionFull { capacity: 4 }).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "session_full");
        assert_eq!(body["capacity"], 4);
    }

    #[tokio::test]
    async fn session_not_startable_returns_409_with_reason() {
        let (status, body) =
            body_json(AppError::SessionNotStartable { reason: "not_host" }).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "session_not_startable");
        assert_eq!(body["reason"], "not_host");
    }
}
