use axum::{
    extract::{Form, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::AsyncCommands;
use rand::Rng;
use std::net::IpAddr;

use crate::state::AppState;
use super::{clear_cookie, require_session, set_cookie, templates, LoginForm, ADMIN_SESSION_TTL_SECS};

fn request_ip(headers: &HeaderMap) -> IpAddr {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| "0.0.0.0".parse().unwrap())
}

pub async fn login_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if require_session(&headers, &state.redis).await.is_some() {
        return Redirect::to("/admin/dashboard").into_response();
    }
    Html(templates::login_page(None)).into_response()
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = request_ip(&headers);
    if state.rl_admin_login.check_key(&ip).is_err() {
        return Html(templates::login_page(Some(
            "Too many attempts. Try again later.",
        )))
        .into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => {
            return Html(templates::login_page(Some("Server error. Try again."))).into_response()
        }
    };

    let stored_hash: Option<String> = conn.get("admin:password_hash").await.unwrap_or(None);
    let Some(hash) = stored_hash else {
        return Html(templates::login_page(Some("Admin not configured."))).into_response();
    };

    let password = form.password.clone();
    let valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &hash).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !valid {
        return Html(templates::login_page(Some("Invalid password."))).into_response();
    }

    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    let token = hex::encode(token_bytes);

    let _: () = conn
        .set_ex(
            format!("admin:session:{}", token),
            "1",
            ADMIN_SESSION_TTL_SECS,
        )
        .await
        .unwrap_or(());

    let mut response = Redirect::to("/admin/dashboard").into_response();
    response.headers_mut().insert(
        "Set-Cookie",
        HeaderValue::from_str(&set_cookie(&token)).unwrap(),
    );
    response
}

/// Activity heartbeat from the admin panel JS. `require_session` already slides
/// the session TTL forward when it validates, so this only has to report whether
/// the session is still alive: 204 keeps the client quiet, 401 tells it to
/// redirect to the login page.
pub async fn keepalive(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if require_session(&headers, &state.redis).await.is_some() {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = require_session(&headers, &state.redis).await {
        if let Ok(mut conn) = state.redis.get().await {
            let _: () = conn
                .del(format!("admin:session:{}", token))
                .await
                .unwrap_or(());
        }
    }
    let mut response = Redirect::to("/admin").into_response();
    response
        .headers_mut()
        .insert("Set-Cookie", HeaderValue::from_static(clear_cookie()));
    response
}
