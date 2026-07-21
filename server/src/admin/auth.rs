use axum::{
    extract::{Form, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::AsyncCommands;
use std::net::IpAddr;

use super::templates::LoginView;
use super::{
    clear_cookie, create_session, oidc, require_session, set_cookie, templates, verify_break_glass,
    AdminRole, LoginForm,
};
use crate::state::AppState;

fn request_ip(headers: &HeaderMap) -> IpAddr {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| "0.0.0.0".parse().unwrap())
}

pub async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if require_session(&headers, &state.redis).await.is_some() {
        return Redirect::to("/admin/dashboard").into_response();
    }

    // Fail-open: no Pocket ID configured ⇒ plain password login, exactly as
    // before OIDC existed.
    if !state.config.oidc_enabled() {
        return Html(templates::login_page(&LoginView {
            oidc_enabled: false,
            show_password: true,
            banner: None,
            error: None,
        }))
        .into_response();
    }

    // OIDC is the front door. The emergency password field only appears when a
    // live probe says Pocket ID is unreachable ("truly down"). The always-on
    // /admin/break-glass route is the backstop if this probe is ever fooled.
    let healthy = oidc::pocket_id_healthy(&state.config.oidc_issuer_url).await;
    let view = if healthy {
        LoginView {
            oidc_enabled: true,
            show_password: false,
            banner: None,
            error: None,
        }
    } else {
        LoginView {
            oidc_enabled: true,
            show_password: true,
            banner: Some("Pocket ID appears unreachable — use the emergency login below."),
            error: None,
        }
    };
    Html(templates::login_page(&view)).into_response()
}

/// GET /admin/break-glass — the always-available emergency password form. The
/// backstop for when the health probe on `/admin` is fooled (Pocket ID's
/// discovery answers but login is actually broken). Deliberately unadvertised.
pub async fn break_glass_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if require_session(&headers, &state.redis).await.is_some() {
        return Redirect::to("/admin/dashboard").into_response();
    }
    Html(templates::login_page(&LoginView {
        oidc_enabled: state.config.oidc_enabled(),
        show_password: true,
        banner: Some("Emergency break-glass login — for use when Pocket ID is unavailable."),
        error: None,
    }))
    .into_response()
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = request_ip(&headers);

    // Re-render the password form with an error, keeping the Pocket ID button
    // visible if OIDC is configured (the user may have reached this via the
    // break-glass page while Pocket ID was actually fine).
    let err_page = |msg: &str| {
        Html(templates::login_page(&LoginView {
            oidc_enabled: state.config.oidc_enabled(),
            show_password: true,
            banner: None,
            error: Some(msg),
        }))
        .into_response()
    };

    if state.rl_admin_login.check_key(&ip).is_err() {
        return err_page("Too many attempts. Try again later.");
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return err_page("Server error. Try again."),
    };

    let stored_hash: Option<String> = conn.get("admin:password_hash").await.unwrap_or(None);
    let Some(hash) = stored_hash else {
        return err_page("Admin not configured.");
    };

    let password = form.password.clone();
    let pepper = state.config.break_glass_pepper.clone();
    let valid = tokio::task::spawn_blocking(move || verify_break_glass(&pepper, &password, &hash))
        .await
        .unwrap_or(false);

    if !valid {
        return err_page("Invalid password.");
    }

    // The break-glass password is SuperAdmin-equivalent — it exists precisely
    // to regain full control when Pocket ID is down.
    let Some(token) =
        create_session(&state.redis, AdminRole::SuperAdmin, "break-glass", "").await
    else {
        // Fail closed: a session that wasn't persisted would "log in" the admin
        // only to bounce them at the next request. Surface the fault instead.
        return err_page("Server error. Try again.");
    };

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

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = require_session(&headers, &state.redis).await {
        if let Ok(mut conn) = state.redis.get().await {
            let _: () = conn
                .del(format!("admin:session:{}", session.token))
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
