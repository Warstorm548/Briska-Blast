//! The Banned Users tools on the Moderation Lists page: **Ban** and **UnBan**.
//!
//! These post as ordinary forms and redirect back to the lists page with a
//! notice, unlike their session-view counterpart in [`super::player`] — that page
//! polls a live transcript and must not navigate, this one does not.
//!
//! # Why a ban from here carries no transcript
//!
//! The session view knows which conversation prompted the ban and pins it. This
//! page has no session context at all, so its records store no `sid` and the
//! ledger's Transcript cell shows an em-dash for them. That is honest: there is
//! no chat to show, as opposed to a chat that was lost.
//!
//! # Reasons here are logging-only
//!
//! Consistent with every other Moderation Lists tool (the 0.30.0 contract): the
//! placeholders read "Reason (logged)", and nothing is sent to the player. The
//! session view's ban is the one that also reaches them, which is why its
//! placeholder says so.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
    Form,
};

use super::super::{chatmod_data, require_session};
use super::blacklist::{lists_redirect, split_words};
use crate::chat::{audit, bans};
use crate::state::AppState;

/// The moderation list these records name. Defined in [`crate::chat::bans`] so
/// this page and the session view cannot drift apart.
use crate::chat::bans::AUDIT_LIST_NAME as LIST_NAME;

/// Form body for the To Ban tool. `ids` and `words` are `;`-separated, matching
/// the separator convention the panel uses everywhere.
#[derive(serde::Deserialize)]
pub struct BanForm {
    #[serde(default)]
    ids: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    words: String,
    #[serde(default)]
    from: Option<String>,
}

/// POST /admin/chatmod/lists/ban
pub async fn lists_ban(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<BanForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match form.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    let ids = split_words(&form.ids);
    if ids.is_empty() {
        return lists_redirect(from.as_deref(), "err", "Enter at least one player ID.");
    }
    let reason = form.reason.trim();
    if reason.is_empty() {
        // Required even though it is never sent to the player: a permanent
        // action with no recorded reason cannot be reviewed later.
        return lists_redirect(from.as_deref(), "err", "Enter a reason — it is logged.");
    }
    let words = split_words(&form.words);

    let Ok(mut conn) = state.redis.get().await else {
        return lists_redirect(from.as_deref(), "err", "Storage unavailable.");
    };

    // Resolve display names before banning so the ledger records who each id was
    // at the time, the same way the session view does.
    let usernames = crate::api::fetch_usernames(&mut conn, &ids).await;
    let now = chrono::Utc::now().timestamp_millis();

    let mut banned = Vec::new();
    let mut already = Vec::new();
    let mut invalid = Vec::new();
    for id in &ids {
        let username = usernames.get(id).cloned().unwrap_or_default();
        let entry = bans::BanEntry {
            player_id: bans::numeric_id(id).unwrap_or_default(),
            username: username.clone(),
            reason: reason.to_string(),
            words: words.clone(),
            banned_by: session.username.clone(),
            banned_sub: session.sub.clone(),
            at_ms: now,
            // No session context from this page — see the module docs.
            sid: String::new(),
            cut_index: 0,
        };
        let outcome = match bans::ban(&mut conn, std::slice::from_ref(id), &entry).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::warn!("chatmod: ban write failed for {}: {}", id, e);
                return lists_redirect(from.as_deref(), "err", "Could not record those bans.");
            }
        };
        invalid.extend(outcome.invalid);
        already.extend(outcome.already);
        if outcome.banned.is_empty() {
            continue;
        }
        banned.extend(outcome.banned);

        // One record per banned player, matching the session view's granularity.
        // A duplicate writes nothing: the original record already holds the
        // reason and evidence, and a second would imply the list changed.
        let record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            "Ban",
            reason,
        )
        .with_target(&username, bans::numeric_id(id))
        .with_words(words.clone())
        // A ban is an action on a player *and* an addition to the ban list. The
        // tag is what surfaces this one record in the List table too, rather
        // than writing a second record that could disagree with it.
        .with_list(LIST_NAME);
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Player, &record).await {
            tracing::warn!("chatmod: ban audit write failed: {}", e);
        }
    }
    drop(conn);

    tracing::info!(
        moderator = %session.username,
        banned = banned.len(),
        "chatmod: ban from moderation lists"
    );

    let msg = ban_summary(&banned, &already, &invalid);
    // Nothing newly banned is not a success — the list is unchanged.
    let key = if banned.is_empty() { "err" } else { "ok" };
    lists_redirect(from.as_deref(), key, &msg)
}

/// Form body for the UnBan tool. `ids` is filled by the confirm dialog from the
/// ticked rows.
#[derive(serde::Deserialize)]
pub struct UnbanForm {
    #[serde(default)]
    ids: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    from: Option<String>,
}

/// POST /admin/chatmod/lists/unban
///
/// Lifting a ban needs no notice to the player: they simply start passing the
/// chat gate again, and the first message they successfully send is the only
/// confirmation that would mean anything.
pub async fn lists_unban(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<UnbanForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let from = match form.from.as_deref() {
        Some(code) => chatmod_data::resolve_live_session(&state, code).await,
        None => None,
    };

    let ids = split_words(&form.ids);
    if ids.is_empty() {
        return lists_redirect(from.as_deref(), "err", "Tick at least one user to un-ban.");
    }
    let reason = form.reason.trim();
    if reason.is_empty() {
        return lists_redirect(from.as_deref(), "err", "Enter a reason — it is logged.");
    }

    let Ok(mut conn) = state.redis.get().await else {
        return lists_redirect(from.as_deref(), "err", "Storage unavailable.");
    };

    // Read the entries before lifting them: once removed there is nothing left to
    // name the player, and an audit record reading "un-banned 000000042" with no
    // username is markedly less useful than one that says who that was.
    let usernames = crate::api::fetch_usernames(&mut conn, &ids).await;

    let lifted = match bans::unban(&mut conn, &ids).await {
        Ok(lifted) => lifted,
        Err(e) => {
            tracing::warn!("chatmod: unban failed: {}", e);
            return lists_redirect(from.as_deref(), "err", "Could not lift those bans.");
        }
    };

    // Un-banning is an action on a player that happens to edit a list, so the
    // record lands in the Player category beside the ban it reverses, tagged so
    // the List table shows the same record. A player's enforcement history and
    // its reversals are then in one log — which is the log any per-player total
    // has to be counted from.
    for id in &lifted {
        let username = usernames
            .get(id)
            // The ticked row supplies the padded id, but a moderator typing the
            // bare number would look the name up under that key instead.
            .or_else(|| bans::numeric_id(id).and_then(|n| usernames.get(&n.to_string())))
            .cloned()
            .unwrap_or_default();
        let record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            "Remove Ban",
            reason,
        )
        .with_target(&username, bans::numeric_id(id))
        .with_list(LIST_NAME);
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Player, &record).await {
            tracing::warn!("chatmod: unban audit write failed: {}", e);
        }
    }
    drop(conn);

    tracing::info!(
        moderator = %session.username,
        lifted = lifted.len(),
        "chatmod: unban from moderation lists"
    );

    let msg = unban_summary(lifted.len(), ids.len());
    // A no-op is not a success, same as an all-duplicate blacklist add.
    let key = if lifted.is_empty() { "err" } else { "ok" };
    lists_redirect(from.as_deref(), key, &msg)
}

/// Human summary of a ban from this page, naming what it skipped rather than
/// reporting a smaller count than the moderator submitted — the same principle
/// as [`super::blacklist::add_summary`].
pub(super) fn ban_summary(banned: &[String], already: &[String], invalid: &[String]) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let mut msg = match banned.len() {
        0 => "Nobody was banned.".to_string(),
        n => format!("Banned {n} player{} from chat.", plural(n)),
    };
    if !already.is_empty() {
        msg.push_str(&format!(" Already banned: {}.", already.join(", ")));
    }
    if !invalid.is_empty() {
        // A player id that is not a number can only be a typo, and silently
        // dropping it would let a moderator believe someone was banned.
        msg.push_str(&format!(" Not a player ID: {}.", invalid.join(", ")));
    }
    msg
}

/// Human summary of an un-ban. `selected` is what the moderator ticked, so a
/// partial lift says so rather than reporting only the number that worked.
pub(super) fn unban_summary(lifted: usize, selected: usize) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match (lifted, selected) {
        (0, _) => "None of those users were banned.".to_string(),
        (n, s) if n == s => format!("Un-banned {n} user{}.", plural(n)),
        (n, s) => format!(
            "Un-banned {n} of {s} selected — the other{} {} not banned.",
            plural(s - n),
            if s - n == 1 { "was" } else { "were" }
        ),
    }
}
