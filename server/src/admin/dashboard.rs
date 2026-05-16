use axum::{
    extract::{Form, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::AsyncCommands;
use semver::Version;
use std::collections::HashMap;

use crate::state::AppState;
use super::{
    require_session,
    templates::{self, DashboardData},
    PasswordForm, VersionForm,
};

pub async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => {
            return Html(format!("Redis error: {e}")).into_response();
        }
    };

    let min_launcher: String = conn
        .get("min_launcher_version")
        .await
        .unwrap_or_else(|_| state.config.min_launcher_version.clone());

    let min_game: String = conn
        .get("min_game_version")
        .await
        .unwrap_or_else(|_| state.config.min_game_version.clone());

    let player_count: u64 = conn.get("player:counter").await.unwrap_or(0u64);

    let session_keys: Vec<String> = redis::cmd("KEYS")
        .arg("session:*")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();
    let session_count = session_keys.len();

    let stored_hash: String = conn.get("admin:password_hash").await.unwrap_or_default();
    let using_default = tokio::task::spawn_blocking(move || {
        bcrypt::verify("@admin", &stored_hash).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    let message = if let Some(ok) = params.get("ok") {
        Some((true, ok.clone()))
    } else {
        params.get("err").map(|err| (false, err.clone()))
    };

    let data = DashboardData {
        min_launcher_version: min_launcher,
        min_game_version: min_game,
        game_port: state.config.game_port,
        admin_port: state.config.admin_port,
        session_count,
        player_count,
        message,
        using_default_password: using_default,
    };

    Html(templates::dashboard_page(&data)).into_response()
}

pub async fn update_launcher_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<VersionForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    if Version::parse(&form.version).is_err() {
        return Redirect::to("/admin/dashboard?err=Invalid+version+format+%28use+1.2.3%29").into_response();
    }
    match state.redis.get().await {
        Ok(mut conn) => {
            let _: () = conn.set("min_launcher_version", &form.version).await.unwrap_or(());
            Redirect::to(&format!("/admin/dashboard?ok=Launcher+version+set+to+{}", form.version)).into_response()
        }
        Err(_) => Redirect::to("/admin/dashboard?err=Redis+error").into_response(),
    }
}

pub async fn update_game_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<VersionForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    if Version::parse(&form.version).is_err() {
        return Redirect::to("/admin/dashboard?err=Invalid+version+format+%28use+1.2.3%29").into_response();
    }
    match state.redis.get().await {
        Ok(mut conn) => {
            let _: () = conn.set("min_game_version", &form.version).await.unwrap_or(());
            Redirect::to(&format!("/admin/dashboard?ok=Game+version+set+to+{}", form.version)).into_response()
        }
        Err(_) => Redirect::to("/admin/dashboard?err=Redis+error").into_response(),
    }
}

pub async fn update_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    if form.new_password != form.confirm_password {
        return Redirect::to("/admin/dashboard?err=New+passwords+do+not+match").into_response();
    }
    if form.new_password.len() < 6 {
        return Redirect::to("/admin/dashboard?err=New+password+must+be+at+least+6+characters").into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/dashboard?err=Redis+error").into_response(),
    };

    let stored_hash: Option<String> = conn.get("admin:password_hash").await.unwrap_or(None);
    let Some(hash) = stored_hash else {
        return Redirect::to("/admin/dashboard?err=Could+not+read+stored+password").into_response();
    };

    let current = form.current_password.clone();
    let valid = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&current, &hash).unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !valid {
        return Redirect::to("/admin/dashboard?err=Current+password+is+incorrect").into_response();
    }

    let new_pw = form.new_password.clone();
    let new_hash = match tokio::task::spawn_blocking(move || {
        bcrypt::hash(&new_pw, bcrypt::DEFAULT_COST)
    })
    .await
    {
        Ok(Ok(h)) => h,
        _ => return Redirect::to("/admin/dashboard?err=Failed+to+hash+new+password").into_response(),
    };

    let _: () = conn.set("admin:password_hash", &new_hash).await.unwrap_or(());
    Redirect::to("/admin/dashboard?ok=Password+updated+successfully").into_response()
}
