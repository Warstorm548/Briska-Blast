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
