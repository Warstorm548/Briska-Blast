use axum::{
    extract::{Form, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use deadpool_redis::redis::{self, AsyncCommands};
use std::collections::HashMap;

use super::{
    require_session,
    templates::{self, UserRow},
};
use crate::api::FREELIST_KEY;
use crate::state::AppState;

/// GET /admin/users — list all known player_ids with their username and
/// dev-flag state. Optional `?q=<term>&field=username|id` narrows the list.
pub async fn users_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => return Html(format!("Redis error: {e}")).into_response(),
    };

    // Mirrors the dashboard's `KEYS session:*` scan. Player set is small in
    // the pre-stable window; revisit with SCAN once it grows.
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("player:*:username")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut users: Vec<UserRow> = Vec::with_capacity(keys.len());
    for key in keys {
        // key format: "player:<id>:username"
        let Some(id) = key
            .strip_prefix("player:")
            .and_then(|s| s.strip_suffix(":username"))
        else {
            continue;
        };
        // Read both as Option<String> so we can distinguish a missing key
        // (the key was deleted between the KEYS scan and our GET — fine, just
        // skip) from a real connection failure (must fail the request).
        let username: Option<String> = match conn.get(&key).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, key = %key, "redis read failed");
                return Redirect::to("/admin/users?err=Redis+error").into_response();
            }
        };
        let Some(username) = username else { continue };
        let raw_flag: Option<String> = match conn.get(format!("player:{}:dev_flag", id)).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, id = %id, "redis read failed");
                return Redirect::to("/admin/users?err=Redis+error").into_response();
            }
        };
        users.push(UserRow {
            id: id.to_string(),
            username,
            dev_flag: raw_flag.as_deref() == Some("true"),
        });
    }

    // Sort by numeric id when possible so 9-digit and legacy 7-digit ids
    // interleave in counter order rather than lexicographic.
    users.sort_by(|a, b| {
        let an = a.id.parse::<u64>().ok();
        let bn = b.id.parse::<u64>().ok();
        match (an, bn) {
            (Some(a), Some(b)) => a.cmp(&b),
            _ => a.id.cmp(&b.id),
        }
    });

    let q = params.get("q").cloned().unwrap_or_default();
    let field = params
        .get("field")
        .cloned()
        .unwrap_or_else(|| "username".into());
    if !q.is_empty() {
        let needle = q.to_lowercase();
        users.retain(|u| match field.as_str() {
            "id" => u.id.contains(&q),
            _ => u.username.to_lowercase().contains(&needle),
        });
    }

    let message = if let Some(ok) = params.get("ok") {
        Some((true, ok.clone()))
    } else {
        params.get("err").map(|err| (false, err.clone()))
    };

    Html(templates::users_page(&users, &q, &field, message)).into_response()
}

/// POST /admin/users/dev-flag — commit checkbox state for every id listed in
/// `known_ids`. Ids not in the form go untouched (defensive against form
/// tampering / partial submission across pages).
pub async fn save_dev_flags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let known: Vec<String> = form
        .get("known_ids")
        .map(|s| {
            s.split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/users?err=Redis+error").into_response(),
    };

    for id in known {
        // Refuse to write dev_flag for an id we have no record of — the
        // hidden known_ids field is user-editable so we re-check existence
        // by looking up the token hash (the canonical "this id exists" key).
        let exists: bool = conn
            .exists(format!("player:{}:token_hash", id))
            .await
            .unwrap_or(false);
        if !exists {
            continue;
        }
        let checked = form
            .get(&format!("dev_{}", id))
            .map(|v| v == "on")
            .unwrap_or(false);
        let value = if checked { "true" } else { "false" };
        if let Err(e) = conn
            .set::<_, _, ()>(format!("player:{}:dev_flag", id), value)
            .await
        {
            tracing::error!(error = %e, id = %id, "failed to set dev_flag");
            return Redirect::to("/admin/users?err=Redis+error").into_response();
        }
    }

    Redirect::to("/admin/users?ok=Saved").into_response()
}

/// POST /admin/users/delete — permanently remove one player: wipe its token,
/// username, and dev-flag, then return the id **number** to the reuse pool
/// (`player:freelist`) so a future registration can reissue it lowest-first.
/// Irreversible; the Users-tab UI gates this behind a confirm modal.
pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if require_session(&headers, &state.redis).await.is_none() {
        return Redirect::to("/admin").into_response();
    }

    let id = form.get("id").map(|s| s.trim().to_string()).unwrap_or_default();
    if id.is_empty() {
        return Redirect::to("/admin/users?err=Missing+id").into_response();
    }

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return Redirect::to("/admin/users?err=Redis+error").into_response(),
    };

    // Re-check existence against the canonical "this id exists" key — the form
    // id is user-supplied, and the player may already be gone (double submit).
    let exists: bool = conn
        .exists(format!("player:{}:token_hash", id))
        .await
        .unwrap_or(false);
    if !exists {
        return Redirect::to("/admin/users?err=No+such+user").into_response();
    }

    // Wipe the complete per-player key set: secret token, username, dev-flag.
    let keys = vec![
        format!("player:{}:token_hash", id),
        format!("player:{}:username", id),
        format!("player:{}:dev_flag", id),
    ];
    if let Err(e) = conn.del::<_, ()>(keys).await {
        tracing::error!(error = %e, id = %id, "failed to delete player keys");
        return Redirect::to("/admin/users?err=Redis+error").into_response();
    }

    // Return the id number to the reuse pool. The id is the zero-padded counter
    // value (a legacy 7-digit id parses the same way); store the bare numeric
    // form as both member and score so ZPOPMIN reissues lowest-first. Best
    // effort: the user is already deleted, so a recycle failure is logged but
    // never fails the request.
    match id.parse::<u64>() {
        Ok(number) => {
            if let Err(e) = conn.zadd::<_, _, _, ()>(FREELIST_KEY, number, number).await {
                tracing::error!(error = %e, id = %id, "failed to add id to freelist");
            }
        }
        Err(_) => tracing::warn!(id = %id, "deleted id is non-numeric; not recycling"),
    }

    tracing::info!("admin deleted player {}", id);
    Redirect::to(&format!("/admin/users?ok=Deleted+{}", id)).into_response()
}
