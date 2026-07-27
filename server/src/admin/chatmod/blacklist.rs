//! The Blacklisted Words tools: add, remove, and the per-word active-filter
//! toggle. Each posts from the Moderation Lists page and redirects back to it,
//! carrying the session context so the moderator lands where they were.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    Form,
};

use super::super::{chatmod_data, require_session};
use crate::chat::{audit, blacklist};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Blacklist
// ---------------------------------------------------------------------------

/// Form body shared by the blacklist tools. `words` is `;`-separated, matching
/// the separator convention the panel uses everywhere. `from` carries the
/// session context so the redirect lands back where the moderator was.
#[derive(serde::Deserialize)]
pub struct BlacklistForm {
    #[serde(default)]
    words: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    active: String,
    #[serde(default)]
    from: Option<String>,
}

/// Split a `;`-separated field into non-empty, trimmed entries.
pub(super) fn split_words(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Where a lists action returns to, preserving `?from=` and appending a notice.
fn lists_redirect(from: Option<&str>, key: &str, msg: &str) -> Response {
    let encoded = urlencoding::encode(msg).into_owned();
    let target = match from {
        Some(code) => format!("/admin/chatmod/lists?from={code}&{key}={encoded}"),
        None => format!("/admin/chatmod/lists?{key}={encoded}"),
    };
    Redirect::to(&target).into_response()
}

/// Add words to the blacklist, write the audit trail, and refresh the cache.
///
/// Shared verbatim by the Moderation Lists page and the session view's Quick
/// Access Tools, which differ only in how they answer: the Lists page redirects
/// with a notice, the session view returns JSON because it must not navigate a
/// moderator away from a live conversation. Keeping the logging here rather than
/// in each caller is the point — two copies of an audit path drift, and a
/// blacklist entry that is missing its record cannot be reconstructed.
///
/// `Err` means the words were not written at all.
pub(super) async fn apply_add(
    state: &AppState,
    session: &crate::admin::AdminSession,
    words: &[String],
    reason: &str,
) -> Result<blacklist::AddOutcome, ()> {
    let Ok(mut conn) = state.redis.get().await else {
        return Err(());
    };
    let outcome = match blacklist::add(&mut conn, words, reason, &session.username).await {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::warn!("chatmod: blacklist add failed: {}", e);
            return Err(());
        }
    };

    // One record per word: the audit log answers "who blacklisted this word and
    // why", which a single record naming several words could not. Duplicates
    // write nothing — the original add is already on the record, and a second
    // entry would imply the list changed when it did not.
    for word in &outcome.added {
        let record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            "Blacklist Word",
            reason,
        )
        .with_words(vec![word.clone()]);
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Word, &record).await {
            tracing::warn!("chatmod: blacklist audit write failed: {}", e);
        }
    }
    drop(conn);
    blacklist::invalidate(state).await;

    Ok(outcome)
}

/// POST /admin/chatmod/lists/blacklist/add
pub async fn blacklist_add(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BlacklistForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match form.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    let words = split_words(&form.words);
    if words.is_empty() {
        return lists_redirect(from.as_deref(), "err", "Enter at least one word.");
    }

    let Ok(outcome) = apply_add(&state, &session, &words, &form.reason).await else {
        return lists_redirect(from.as_deref(), "err", "Could not add those words.");
    };

    let msg = add_summary(&outcome.added, &outcome.duplicates);
    // All-duplicate is not a success — nothing changed, and saying "Added 0
    // words" in a green banner reads as though it worked.
    let key = if outcome.added.is_empty() { "err" } else { "ok" };
    lists_redirect(from.as_deref(), key, &msg)
}

/// Human summary of an add, naming the words that were already listed rather
/// than just reporting a smaller count than the moderator submitted.
pub(super) fn add_summary(added: &[String], duplicates: &[String]) -> String {
    let plural = |n: usize| if n == 1 { "word" } else { "words" };
    match (added.len(), duplicates.len()) {
        (0, 0) => "Nothing to add.".to_string(),
        (0, d) => format!(
            "No new words — {} already listed: {}.",
            if d == 1 { "it is" } else { "they are" },
            duplicates.join(", ")
        ),
        (a, 0) => format!("Added {a} {}.", plural(a)),
        (a, d) => format!(
            "Added {a} {}. {d} already listed: {}.",
            plural(a),
            duplicates.join(", ")
        ),
    }
}

/// POST /admin/chatmod/lists/blacklist/remove
pub async fn blacklist_remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BlacklistForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match form.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    let words = split_words(&form.words);
    if words.is_empty() {
        return lists_redirect(from.as_deref(), "err", "Enter at least one word.");
    }

    let Ok(mut conn) = state.redis.get().await else {
        return lists_redirect(from.as_deref(), "err", "Storage unavailable.");
    };
    let removed = match blacklist::remove(&mut conn, &words).await {
        Ok(removed) => removed,
        Err(e) => {
            tracing::warn!("chatmod: blacklist remove failed: {}", e);
            return lists_redirect(from.as_deref(), "err", "Could not remove those words.");
        }
    };

    for word in &removed {
        let record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            "Remove Blacklist Word",
            &form.reason,
        )
        .with_words(vec![word.clone()]);
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Word, &record).await {
            tracing::warn!("chatmod: blacklist audit write failed: {}", e);
        }
    }
    drop(conn);
    blacklist::invalidate(&state).await;

    let msg = match removed.len() {
        0 => "None of those words were on the list.".to_string(),
        1 => "Removed 1 word.".to_string(),
        n => format!("Removed {n} words."),
    };
    // A no-op is not a success, same as an all-duplicate add: nothing changed,
    // and a green banner would read as though it had.
    let key = if removed.is_empty() { "err" } else { "ok" };
    lists_redirect(from.as_deref(), key, &msg)
}

/// POST /admin/chatmod/lists/blacklist/toggle — flip a word's Active Filter.
///
/// A disabled word stays on the list, and stays in the audit history, but stops
/// matching. That is deliberately different from removing it.
pub async fn blacklist_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BlacklistForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match form.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    let Some(word) = split_words(&form.words).into_iter().next() else {
        return lists_redirect(from.as_deref(), "err", "No word given.");
    };
    let active = form.active == "1";

    let Ok(mut conn) = state.redis.get().await else {
        return lists_redirect(from.as_deref(), "err", "Storage unavailable.");
    };
    match blacklist::set_active(&mut conn, &word, active).await {
        Ok(true) => {}
        Ok(false) => return lists_redirect(from.as_deref(), "err", "That word is not on the list."),
        Err(e) => {
            tracing::warn!("chatmod: blacklist toggle failed: {}", e);
            return lists_redirect(from.as_deref(), "err", "Could not update that word.");
        }
    }

    let action = if active { "Enable Word Filter" } else { "Disable Word Filter" };
    let record = audit::AuditRecord::by_moderator(
        &session.username,
        &session.sub,
        session.role,
        action,
        &form.reason,
    )
    .with_words(vec![word.clone()]);
    if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Word, &record).await {
        tracing::warn!("chatmod: blacklist audit write failed: {}", e);
    }
    drop(conn);
    blacklist::invalidate(&state).await;

    let msg = if active {
        format!("Filter enabled for \"{word}\".")
    } else {
        format!("Filter disabled for \"{word}\".")
    };
    lists_redirect(from.as_deref(), "ok", &msg)
}
