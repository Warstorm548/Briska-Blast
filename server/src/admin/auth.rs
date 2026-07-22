use axum::{
    extract::{Form, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::AsyncCommands;
use std::net::IpAddr;

use super::templates::LoginView;
use super::{
    clear_cookie, create_session, hash_break_glass, oidc, require_session, set_cookie, templates,
    verify_break_glass, AdminRole, LoginForm, PasswordForm, PASSWORD_MUST_ROTATE_KEY,
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
        // Record the abuse signal keyed by a pseudonymized IP — enough to see
        // one client hammering login without ever writing a raw address.
        tracing::warn!(
            client = %crate::redact::hash_ip_token(&state.ip_hash_key, &ip.to_string()),
            "admin login rate-limited"
        );
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

    // A seeded, publicly-known default password must never open a SuperAdmin
    // session — route it through forced rotation instead. The marker is set at
    // seed only for the literal default and cleared once rotated.
    let must_rotate: bool = conn.exists(PASSWORD_MUST_ROTATE_KEY).await.unwrap_or(false);
    if must_rotate {
        return Redirect::to("/admin/force-password").into_response();
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

/// GET /admin/force-password — the forced-rotation form, shown only while the
/// seeded default password is still in place. The default can prove identity
/// (to change itself) but never open a session, so this is reachable without
/// one; if nothing needs rotating it just bounces to the login page.
pub async fn force_password_page(State(state): State<AppState>) -> Response {
    let pending = match state.redis.get().await {
        Ok(mut conn) => conn.exists(PASSWORD_MUST_ROTATE_KEY).await.unwrap_or(false),
        Err(_) => false,
    };
    if !pending {
        return Redirect::to("/admin").into_response();
    }
    Html(templates::force_password_page(None)).into_response()
}

/// POST /admin/force-password — replace the seeded default. Verifies the current
/// (default) password, stores the new one, clears the must-rotate marker, and
/// only then mints a SuperAdmin session. No-ops (redirects to login) once the
/// marker is gone, so it can't be used as a general password-reset endpoint.
pub async fn force_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    let ip = request_ip(&headers);
    let err_page = |msg: &str| Html(templates::force_password_page(Some(msg))).into_response();

    if state.rl_admin_login.check_key(&ip).is_err() {
        return err_page("Too many attempts. Try again later.");
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return err_page("Server error. Try again."),
    };

    // Only meaningful while the default is pending rotation.
    let pending: bool = conn.exists(PASSWORD_MUST_ROTATE_KEY).await.unwrap_or(false);
    if !pending {
        return Redirect::to("/admin").into_response();
    }

    if form.new_password != form.confirm_password {
        return err_page("New passwords do not match.");
    }
    if form.new_password.len() < 6 {
        return err_page("New password must be at least 6 characters.");
    }
    if form.new_password == "@admin" {
        return err_page("Choose a password other than the default.");
    }

    let stored_hash: Option<String> = conn.get("admin:password_hash").await.unwrap_or(None);
    let Some(hash) = stored_hash else {
        return err_page("Admin not configured.");
    };

    let current = form.current_password.clone();
    let pepper = state.config.break_glass_pepper.clone();
    let valid = tokio::task::spawn_blocking(move || verify_break_glass(&pepper, &current, &hash))
        .await
        .unwrap_or(false);
    if !valid {
        return err_page("Current password is incorrect.");
    }

    let new_pw = form.new_password.clone();
    let pepper = state.config.break_glass_pepper.clone();
    let new_hash = match tokio::task::spawn_blocking(move || hash_break_glass(&pepper, &new_pw)).await
    {
        Ok(Ok(h)) => h,
        _ => return err_page("Failed to hash new password."),
    };

    if conn
        .set::<_, _, ()>("admin:password_hash", &new_hash)
        .await
        .is_err()
    {
        return err_page("Server error. Try again.");
    }
    let _: () = conn.del(PASSWORD_MUST_ROTATE_KEY).await.unwrap_or(());

    // Rotated — the credential is no longer the public default, so it's now safe
    // to grant the SuperAdmin session break-glass is meant to provide.
    let Some(token) =
        create_session(&state.redis, AdminRole::SuperAdmin, "break-glass", "").await
    else {
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
