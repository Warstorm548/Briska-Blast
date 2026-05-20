use axum::response::Html;

/// Same-origin HTML/JS harness for manual WebRTC signaling verification.
///
/// Only registered as a route when the `ENABLE_TEST_HARNESS` env var is
/// set to `true`. Default off — this is a debugging tool and shouldn't
/// be reachable from production.
pub async fn page() -> Html<&'static str> {
    Html(include_str!("page.html"))
}
