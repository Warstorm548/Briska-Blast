use axum::{
    extract::{Form, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use chrono::NaiveDateTime;
use deadpool_redis::redis::AsyncCommands;
use semver::Version;
use std::collections::HashMap;

use crate::state::AppState;
use crate::update::UpdateCommand;
use super::{
    require_session,
    templates::{self, DashboardData},
    PasswordForm, RollbackForm, ScheduleUpdateForm, UpdateSettingsForm, VersionForm,
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

    // update state
    let update_auto_enabled: String = conn.get("update:auto_enabled").await.unwrap_or_default();
    let update_check_interval: String = conn.get("update:check_interval_secs").await.unwrap_or_default();
    let update_apply_interval: String = conn.get("update:apply_interval_secs").await.unwrap_or_default();
    let update_available: String = conn.get("update:available_version").await.unwrap_or_default();
    let update_last_checked: String = conn.get("update:last_checked").await.unwrap_or_default();
    let update_scheduled_at: String = conn.get("update:scheduled_at").await.unwrap_or_default();
    let update_scheduled_version: String = conn.get("update:scheduled_version").await.unwrap_or_default();
    let update_manual_override: String = conn.get("update:manual_override").await.unwrap_or_default();
    let update_previous_version: String = conn.get("update:previous_version").await.unwrap_or_default();
    let update_rollback_locked: String = conn.get("update:rollback_locked").await.unwrap_or_default();

    let fmt_ts = |ts: &str| -> Option<String> {
        ts.parse::<i64>().ok().and_then(|t| {
            chrono::DateTime::from_timestamp(t, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        })
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
        release_channel: env!("RELEASE_CHANNEL"),
        server_version: env!("CARGO_PKG_VERSION"),
        update_last_checked: fmt_ts(&update_last_checked),
        update_available: if update_available.is_empty() { None } else { Some(update_available) },
        update_auto_enabled: update_auto_enabled == "true",
        update_check_interval_secs: update_check_interval.parse().unwrap_or(21600),
        update_apply_interval_secs: update_apply_interval.parse().ok().filter(|&v| v > 0u64),
        update_scheduled_at: fmt_ts(&update_scheduled_at),
        update_scheduled_version: if update_scheduled_version.is_empty() { None } else { Some(update_scheduled_version) },
        _update_manual_override: update_manual_override == "true",
        update_previous_version: if update_previous_version.is_empty() { None } else { Some(update_previous_version) },
        update_rollback_locked: update_rollback_locked == "true",
    };

    Html(templates::dashboard_page(&data)).into_response()}

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

pub async fn check_for_update(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    let client = reqwest::Client::new();
    let channel = env!("RELEASE_CHANNEL");
    let now = chrono::Utc::now().timestamp();

    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.set("update:last_checked", now.to_string()).await.unwrap_or(());
    }

    match crate::update::github::check_for_update(&client, channel).await {
        Some(tag) => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.set("update:available_version", &tag).await.unwrap_or(());
                let _: () = conn.set("update:found_at", now.to_string()).await.unwrap_or(());
            }
            Redirect::to(&format!("/admin/dashboard?ok=Update+available%3A+{}", tag)).into_response()
        }
        None => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.del("update:available_version").await.unwrap_or(());
                let _: () = conn.del("update:found_at").await.unwrap_or(());
            }
            Redirect::to("/admin/dashboard?ok=Already+up+to+date").into_response()
        }
    }
}

pub async fn apply_update_now(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    if let Ok(mut conn) = state.redis.get().await {
        let current = env!("CARGO_PKG_VERSION");
        let _: () = conn.set("update:previous_version", current).await.unwrap_or(());
    }
    let _ = state.update_tx.send(UpdateCommand::ApplyNow).await;
    Redirect::to("/admin/dashboard?ok=Update+triggered").into_response()
}

pub async fn schedule_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ScheduleUpdateForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    // parse "2026-05-20T03:00" from datetime-local
    let ts = NaiveDateTime::parse_from_str(&form.scheduled_at, "%Y-%m-%dT%H:%M")
        .map(|dt| dt.and_utc().timestamp());

    match ts {
        Ok(ts) => {
            if let Ok(mut conn) = state.redis.get().await {
                let available: String = conn.get("update:available_version").await.unwrap_or_default();
                let _: () = conn.set("update:scheduled_at", ts.to_string()).await.unwrap_or(());
                let _: () = conn.set("update:scheduled_version", &available).await.unwrap_or(());
            }
            let _ = state.update_tx.send(UpdateCommand::Schedule(ts)).await;
            Redirect::to("/admin/dashboard?ok=Update+scheduled").into_response()
        }
        Err(_) => Redirect::to("/admin/dashboard?err=Invalid+date+format").into_response(),
    }
}

pub async fn cancel_update(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    let _ = state.update_tx.send(UpdateCommand::CancelSchedule).await;
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.del("update:scheduled_at").await.unwrap_or(());
        let _: () = conn.del("update:scheduled_version").await.unwrap_or(());
        let _: () = conn.del("update:previous_version").await.unwrap_or(());
    }
    Redirect::to("/admin/dashboard?ok=Scheduled+update+cancelled").into_response()
}

pub async fn save_update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<UpdateSettingsForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }
    let auto_enabled = form.auto_enabled.as_deref() == Some("on");
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.set("update:auto_enabled", if auto_enabled { "true" } else { "false" }).await.unwrap_or(());
        if auto_enabled {
            // re-enabling auto-update clears the rollback safety lock
            let _: () = conn.del("update:rollback_locked").await.unwrap_or(());
        }
        if let Some(v) = &form.check_interval_secs {
            if v.parse::<u64>().is_ok() {
                let _: () = conn.set("update:check_interval_secs", v).await.unwrap_or(());
            }
        }
        if let Some(v) = &form.apply_interval_secs {
            let _: () = conn.set("update:apply_interval_secs", v).await.unwrap_or(());
        }
    }
    let _ = state.update_tx.send(UpdateCommand::SettingsChanged).await;
    Redirect::to("/admin/dashboard?ok=Update+settings+saved").into_response()
}

pub async fn rollback_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<RollbackForm>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let channel = env!("RELEASE_CHANNEL");
    let version = form.version.trim_start_matches('v');
    let versioned_tag = format!("v{}", version);

    match crate::update::docker::retag_for_rollback(&versioned_tag, channel).await {
        Ok(()) => {
            if let Ok(mut conn) = state.redis.get().await {
                // safety lock: disable auto-update after rollback
                let _: () = conn.set("update:auto_enabled", "false").await.unwrap_or(());
                let _: () = conn.set("update:rollback_locked", "true").await.unwrap_or(());
                let _: () = conn.del("update:previous_version").await.unwrap_or(());
                let _: () = conn.del("update:available_version").await.unwrap_or(());
                let _: () = conn.del("update:found_at").await.unwrap_or(());
            }
            let _ = state.update_tx.send(UpdateCommand::SettingsChanged).await;
            // trigger Watchtower to restart with the retagged image
            let client = reqwest::Client::new();
            let ok = crate::update::watchtower::trigger_update(
                &client,
                &state.config.watchtower_url,
                &state.config.watchtower_token,
            ).await;
            if ok {
                Redirect::to("/admin/dashboard?ok=Rollback+triggered+%E2%80%94+auto-update+disabled").into_response()
            } else {
                // Watchtower failed — undo Redis state so nothing is left half-applied
                if let Ok(mut conn) = state.redis.get().await {
                    let _: () = conn.del("update:rollback_locked").await.unwrap_or(());
                    let _: () = conn.set("update:auto_enabled", "false").await.unwrap_or(());
                }
                Redirect::to("/admin/dashboard?err=Rollback+failed%3A+Watchtower+did+not+respond").into_response()
            }
        }
        Err(e) => {
            Redirect::to(&format!("/admin/dashboard?err=Rollback+failed%3A+{}", urlencoding(&e))).into_response()
        }
    }
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "+").replace(':', "%3A")
}
