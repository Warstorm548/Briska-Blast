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

// Allowed values for the auto-update interval dropdowns. Anything outside
// these sets is rejected — prevents tampered POSTs from setting e.g.
// check_interval=1s (would DoS GitHub) or apply_interval to a malformed
// value that parses to "apply immediately".
const ALLOWED_CHECK_INTERVALS: &[u64] = &[21600, 43200, 86400, 172800];
const ALLOWED_APPLY_INTERVALS: &[u64] = &[0, 86400, 259200, 604800, 1209600];
use super::{
    hash_break_glass, require_role, require_session, verify_break_glass,
    templates::{self, DashboardData},
    AdminRole, PasswordForm, RollbackForm, ScheduleUpdateForm, UpdateSettingsForm, VersionForm,
};

pub async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // Any authenticated role (Moderator and up) may view the dashboard; the
    // template gates the operational sections by role.
    let session = match require_session(&headers, &state.redis).await {
        Some(s) => s,
        None => return Redirect::to("/admin").into_response(),
    };

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
    let pepper = state.config.break_glass_pepper.clone();
    let using_default = tokio::task::spawn_blocking(move || {
        verify_break_glass(&pepper, "@admin", &stored_hash)
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
        role: session.role,
        username: session.username,
        release_channel: env!("RELEASE_CHANNEL"),
        server_version: env!("CARGO_PKG_VERSION"),
        update_last_checked: fmt_ts(&update_last_checked),
        // Defense-in-depth against the post-apply stuck-banner bug
        // (root-cause fix is in update::task::run, which clears stale
        // state on startup). If Redis somehow still holds an
        // available_version we already are or are past, render as if
        // it weren't set. Banner only shows for genuinely-newer tags.
        update_available: if update_available.is_empty()
            || crate::update::task::is_stale_available_version(
                &update_available,
                env!("CARGO_PKG_VERSION"),
            )
        {
            None
        } else {
            Some(update_available)
        },
        update_auto_enabled: update_auto_enabled == "true",
        update_check_interval_secs: update_check_interval.parse().unwrap_or(21600),
        update_apply_interval_secs: update_apply_interval.parse().ok().filter(|&v| v > 0u64),
        update_scheduled_at: fmt_ts(&update_scheduled_at),
        update_scheduled_version: if update_scheduled_version.is_empty() { None } else { Some(update_scheduled_version) },
        update_previous_version: if update_previous_version.is_empty() { None } else { Some(update_previous_version) },
        update_rollback_locked: update_rollback_locked == "true",
    };

    Html(templates::dashboard_page(&data)).into_response()}

pub async fn update_launcher_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<VersionForm>,
) -> Response {
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
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
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
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
    // Changing the break-glass password is SuperAdmin-only — it's the
    // credential that grants SuperAdmin login, so it sits with identity control.
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::SuperAdmin).await {
        return resp;
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
    let pepper = state.config.break_glass_pepper.clone();
    let valid = tokio::task::spawn_blocking(move || {
        verify_break_glass(&pepper, &current, &hash)
    })
    .await
    .unwrap_or(false);

    if !valid {
        return Redirect::to("/admin/dashboard?err=Current+password+is+incorrect").into_response();
    }

    let new_pw = form.new_password.clone();
    let pepper = state.config.break_glass_pepper.clone();
    let new_hash = match tokio::task::spawn_blocking(move || {
        hash_break_glass(&pepper, &new_pw)
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
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/dashboard?err=Internal+HTTP+client+error").into_response(),
    };
    let channel = env!("RELEASE_CHANNEL");
    let now = chrono::Utc::now().timestamp();

    use crate::update::github::CheckOutcome;
    match crate::update::github::check_for_update(&client, channel, &state.redis).await {
        Ok(CheckOutcome::Update(tag)) => {
            if let Ok(mut conn) = state.redis.get().await {
                // Only reset found_at when this is a new version we haven't seen before.
                let prev: String = conn.get("update:available_version").await.unwrap_or_default();
                let _: () = conn.set("update:available_version", &tag).await.unwrap_or(());
                if prev != tag {
                    let _: () = conn.set("update:found_at", now.to_string()).await.unwrap_or(());
                }
                let _: () = conn.set("update:last_checked", now.to_string()).await.unwrap_or(());
            }
            Redirect::to(&format!("/admin/dashboard?ok=Update+available%3A+{}", urlencoding::encode(&tag))).into_response()
        }
        Ok(CheckOutcome::NoUpdate) => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.del("update:available_version").await.unwrap_or(());
                let _: () = conn.del("update:found_at").await.unwrap_or(());
                let _: () = conn.set("update:last_checked", now.to_string()).await.unwrap_or(());
            }
            Redirect::to("/admin/dashboard?ok=Already+up+to+date").into_response()
        }
        Ok(CheckOutcome::NotModified) => {
            // GitHub returned 304 — cached state is still correct, but the
            // check itself did happen so the timestamp is accurate.
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.set("update:last_checked", now.to_string()).await.unwrap_or(());
            }
            Redirect::to("/admin/dashboard?ok=No+changes+since+last+check").into_response()
        }
        Err(_) => {
            // Network/parse failure — preserve previously detected update AND
            // leave last_checked untouched so the dashboard doesn't show a
            // fresh timestamp for a check that never actually succeeded.
            Redirect::to("/admin/dashboard?err=Check+failed%3A+could+not+reach+GitHub").into_response()
        }
    }
}

pub async fn apply_update_now(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }
    // Precondition: there must actually be an update available. The UI hides
    // the button otherwise, but a tampered POST would otherwise re-trigger
    // Watchtower against the same channel image.
    if let Ok(mut conn) = state.redis.get().await {
        let available: String = conn.get("update:available_version").await.unwrap_or_default();
        if available.is_empty() {
            return Redirect::to("/admin/dashboard?err=No+update+available").into_response();
        }
        // Also bail if a rollback safety lock is in place — re-enabling
        // auto-update (which clears the lock) is the documented unlock path.
        let rb: String = conn.get("update:rollback_locked").await.unwrap_or_default();
        if rb == "true" {
            return Redirect::to("/admin/dashboard?err=Rollback+lock+is+set+%E2%80%94+re-enable+auto-update+to+clear+it").into_response();
        }
    }
    // NOTE: `update:previous_version` is now written by the update task only
    // after Watchtower actually accepts the trigger. Writing it eagerly here
    // left a stale rollback target if the trigger failed.
    let _ = state.update_tx.send(UpdateCommand::ApplyNow).await;
    Redirect::to("/admin/dashboard?ok=Update+triggered").into_response()
}

pub async fn schedule_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ScheduleUpdateForm>,
) -> Response {
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }
    // parse "2026-05-20T03:00" from datetime-local
    let ts = NaiveDateTime::parse_from_str(&form.scheduled_at, "%Y-%m-%dT%H:%M")
        .map(|dt| dt.and_utc().timestamp());

    let ts = match ts {
        Ok(ts) => ts,
        Err(_) => return Redirect::to("/admin/dashboard?err=Invalid+date+format").into_response(),
    };

    // Fail closed: any Redis error must surface as an err redirect rather
    // than fall through to the success path. Otherwise the dashboard would
    // show "Update scheduled" while `scheduled_at`/`scheduled_version` were
    // never persisted, leaving the apply path with nothing to run.
    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/dashboard?err=Redis+error").into_response(),
    };

    let available: String = match conn.get("update:available_version").await {
        Ok(v) => v,
        Err(_) => return Redirect::to("/admin/dashboard?err=Redis+error").into_response(),
    };
    // Mirror the precondition in `apply_update_now`: refuse to persist a
    // schedule when there is no actual target. Otherwise a stale `scheduled_at`
    // would later spawn a `wait_and_apply` with an empty `scheduled_version`.
    if available.is_empty() {
        return Redirect::to("/admin/dashboard?err=No+update+available").into_response();
    }

    if conn
        .set::<_, _, ()>("update:scheduled_at", ts.to_string())
        .await
        .is_err()
    {
        return Redirect::to("/admin/dashboard?err=Redis+error").into_response();
    }
    if conn
        .set::<_, _, ()>("update:scheduled_version", &available)
        .await
        .is_err()
    {
        // scheduled_at was already written — clear it so we don't leave a
        // half-persisted schedule that would spawn a `wait_and_apply` with no
        // version target.
        let _: Result<(), _> = conn.del("update:scheduled_at").await;
        return Redirect::to("/admin/dashboard?err=Redis+error").into_response();
    }

    let _ = state.update_tx.send(UpdateCommand::Schedule(ts)).await;
    Redirect::to("/admin/dashboard?ok=Update+scheduled").into_response()
}

pub async fn cancel_update(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }
    let _ = state.update_tx.send(UpdateCommand::CancelSchedule).await;
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.del("update:scheduled_at").await.unwrap_or(());
        let _: () = conn.del("update:scheduled_version").await.unwrap_or(());
        // NOTE: do NOT delete update:previous_version here — it is the rollback
        // target from the last actually-applied update, not state owned by the
        // pending schedule.
    }
    Redirect::to("/admin/dashboard?ok=Scheduled+update+cancelled").into_response()
}

pub async fn save_update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<UpdateSettingsForm>,
) -> Response {
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }
    let auto_enabled = form.auto_enabled.as_deref() == Some("on");
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.set("update:auto_enabled", if auto_enabled { "true" } else { "false" }).await.unwrap_or(());
        if auto_enabled {
            // re-enabling auto-update clears the rollback safety lock
            let _: () = conn.del("update:rollback_locked").await.unwrap_or(());
        }
        if let Some(v) = &form.check_interval_secs {
            if let Ok(n) = v.parse::<u64>() {
                if ALLOWED_CHECK_INTERVALS.contains(&n) {
                    let _: () = conn.set("update:check_interval_secs", v).await.unwrap_or(());
                }
            }
        }
        if let Some(v) = &form.apply_interval_secs {
            if let Ok(n) = v.parse::<u64>() {
                if ALLOWED_APPLY_INTERVALS.contains(&n) {
                    let _: () = conn.set("update:apply_interval_secs", v).await.unwrap_or(());
                }
            }
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
    if let Err(resp) = require_role(&headers, &state.redis, AdminRole::Admin).await {
        return resp;
    }

    let channel = env!("RELEASE_CHANNEL");
    let version = form.version.trim_start_matches('v').to_string();

    // Validate as semver — the hidden form field is cosmetic; the value lands
    // in a Docker image tag, so it MUST parse. Rejects arbitrary tag input
    // (e.g. "latest", "main", malicious-tag-with-dotdot).
    if Version::parse(&version).is_err() {
        return Redirect::to("/admin/dashboard?err=Invalid+rollback+version+format").into_response();
    }

    // Cross-check against the authoritative rollback target in Redis. A
    // tampered POST that asks to deploy some other historical tag is rejected
    // even though the admin session is valid — Redis is the source of truth,
    // not the form field.
    if let Ok(mut conn) = state.redis.get().await {
        let stored: String = conn.get("update:previous_version").await.unwrap_or_default();
        let stored_trim = stored.trim_start_matches('v').to_string();
        if stored_trim.is_empty() || stored_trim != version {
            return Redirect::to("/admin/dashboard?err=Rollback+target+does+not+match+stored+previous+version").into_response();
        }
    } else {
        return Redirect::to("/admin/dashboard?err=Redis+error").into_response();
    }

    let versioned_tag = format!("v{}", version);

    // Acquire the apply lock BEFORE retag_for_rollback. The retag mutates the
    // local Docker `:channel` ref, and so does `maybe_apply` / `wait_and_apply`
    // / `ApplyNow` via `pull_channel_image`. Both must serialise — v0.4.2
    // closed the read side of this race (auto-apply re-reads state under the
    // lock); v0.4.3 closes the write side by widening the lock to cover the
    // retag itself.
    let _guard = state.update_apply_lock.lock().await;

    // Build the HTTP client BEFORE the retag. If the builder fails, we'd
    // otherwise leave the local `:channel` ref rewritten with no Watchtower
    // trigger and no Redis bookkeeping — a worse state than not having tried.
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/dashboard?err=Internal+HTTP+client+error").into_response(),
    };

    match crate::update::docker::retag_for_rollback(&versioned_tag, channel).await {
        Ok(()) => {
            // Trigger Watchtower FIRST. Only commit Redis state changes if it
            // actually succeeds — otherwise we'd lose the rollback target
            // (previous_version) and the cached "update available" state
            // while the rollback never actually happened.
            let ok = crate::update::watchtower::trigger_update(
                &client,
                &state.config.watchtower_url,
                &state.config.watchtower_token,
            ).await;
            if ok {
                if let Ok(mut conn) = state.redis.get().await {
                    // safety lock: disable auto-update after rollback
                    let _: () = conn.set("update:auto_enabled", "false").await.unwrap_or(());
                    let _: () = conn.set("update:rollback_locked", "true").await.unwrap_or(());
                    let _: () = conn.del("update:previous_version").await.unwrap_or(());
                    let _: () = conn.del("update:available_version").await.unwrap_or(());
                    let _: () = conn.del("update:found_at").await.unwrap_or(());
                }
                let _ = state.update_tx.send(UpdateCommand::SettingsChanged).await;
                Redirect::to("/admin/dashboard?ok=Rollback+triggered+%E2%80%94+auto-update+disabled").into_response()
            } else {
                // Watchtower didn't accept the trigger. The local retag DOES
                // persist (Watchtower runs with `WATCHTOWER_NO_PULL=true`, so
                // Watchtower itself will never undo a local retag — see
                // containrrr discussion #557), but our own auto-apply paths
                // (`maybe_apply` / `wait_and_apply` / `ApplyNow`) call
                // `pull_channel_image` from the registry before triggering
                // Watchtower and WOULD overwrite the persisted retag. Set the
                // rollback safety lock so the next auto-apply bails out under
                // `should_apply_after_lock`. The admin can retry the rollback
                // (handler is idempotent on the local retag) or roll forward
                // by re-enabling auto-update.
                //
                // We intentionally do NOT delete `previous_version`,
                // `available_version`, or `found_at` here — the rollback did
                // not complete, so those values remain valid for a retry.
                if let Ok(mut conn) = state.redis.get().await {
                    let _: () = conn.set("update:auto_enabled", "false").await.unwrap_or(());
                    let _: () = conn.set("update:rollback_locked", "true").await.unwrap_or(());
                }
                let _ = state.update_tx.send(UpdateCommand::SettingsChanged).await;
                Redirect::to("/admin/dashboard?err=Rollback+failed%3A+Watchtower+did+not+respond+%E2%80%94+auto-update+disabled%2C+retry+rollback+or+re-enable+to+roll+forward").into_response()
            }
        }
        Err(e) => {
            Redirect::to(&format!("/admin/dashboard?err=Rollback+failed%3A+{}", urlencoding::encode(&e))).into_response()
        }
    }
}
