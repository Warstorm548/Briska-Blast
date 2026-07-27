//! Player moderation tools: **Warn Only** and **Warn + Delete Chat Body**.
//!
//! Both post from the session view and answer with JSON rather than a redirect,
//! because that page polls its transcript every two seconds — navigating away
//! and back would throw away the moderator's place in a live conversation. That
//! is the only reason these differ from the Moderation Lists tools.
//!
//! # What a warning is
//!
//! A one-off notice to a single player, carrying the reason the moderator typed.
//! It is not a privilege change: unlike Suspend and Ban, nothing about the
//! player's account changes, so there is no state to lift later.
//!
//! Delivery is **live-only and never queued**. A player who has disconnected, or
//! who is in a match (chat renders only in the lobby), simply does not receive
//! it; the attempt is recorded as undelivered on both the audit record and the
//! transcript echo, and the moderator is told. Holding warnings for later would
//! deliver them detached from the conversation that prompted them.
//!
//! # What deletion is
//!
//! The bodies vanish from every connected player's chat. Server-side **nothing
//! is removed** — see [`crate::chat::transcript::DeletionMark`]. The moderator
//! keeps seeing the message, greyed, because the panel reads the transcript,
//! which is append-only.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form, Json,
};

use super::super::{chatmod_data, require_session};
use super::blacklist::split_words;
use crate::chat::{audit, transcript, MAX_CHAT_LEN};
use crate::signaling::protocol::ServerMsg;
use crate::state::AppState;

/// Form body for both warn buttons. `targets` and `body_ids` are `;`-separated,
/// matching the separator convention the panel uses everywhere. `delete` is `1`
/// for Warn + Delete Chat Body and absent for Warn Only.
#[derive(serde::Deserialize)]
pub struct WarnForm {
    #[serde(default)]
    targets: String,
    #[serde(default)]
    body_ids: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    delete: String,
}

/// What the panel renders as a notice above the tools.
#[derive(serde::Serialize)]
struct WarnReply {
    ok: bool,
    msg: String,
}

fn reply(status: StatusCode, ok: bool, msg: impl Into<String>) -> Response {
    (
        status,
        Json(WarnReply {
            ok,
            msg: msg.into(),
        }),
    )
        .into_response()
}

fn bad(msg: impl Into<String>) -> Response {
    reply(StatusCode::BAD_REQUEST, false, msg)
}

/// Why a warning did not reach someone. Phrased for a moderator reading a notice.
const OFFLINE: &str = "not connected";
const IN_MATCH: &str = "in an active match";

/// POST /admin/chatmod/session/:code/warn
///
/// One audit record per **(action instance, target player)** — a press hitting
/// three players writes three records, each carrying the bodies that player sent.
/// Repeated presses are never merged: a player may be warned several times in one
/// session and each is its own row.
pub async fn chatmod_warn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Form(form): Form<WarnForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let Some(canon) = chatmod_data::resolve_live_session(&state, &code).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // The reason reaches the player, so it is bounded like any other line that
    // does. It is deliberately not censored — a moderator explaining why a word
    // was a problem should be able to name it.
    let reason = form.reason.trim();
    if reason.is_empty() {
        return bad("Enter a reason — it is sent to the player.");
    }
    let reason: String = reason.chars().take(MAX_CHAT_LEN).collect();

    let targets = split_words(&form.targets);
    if targets.is_empty() {
        return bad("Enter at least one target player ID.");
    }
    let selected = split_words(&form.body_ids);
    let deleting = form.delete == "1";
    if deleting && selected.is_empty() {
        return bad("Tick at least one message to delete, or use Warn Only.");
    }

    let Ok(mut conn) = state.redis.get().await else {
        return reply(StatusCode::SERVICE_UNAVAILABLE, false, "Storage unavailable.");
    };

    // No transcript means no chat happened here: there is nothing to delete and
    // no conversation to pin a snapshot to. A warning still sends.
    let sid = transcript::live_sid(&mut conn, &canon)
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    if deleting && sid.is_empty() {
        return bad("This session has no chat history to delete from.");
    }

    // Derive which body belongs to which sender from the stored transcript, not
    // from the form. The client could name any pairing it liked; the server
    // already knows the truth, and an audit record that misattributes a message
    // is worse than one that omits it.
    let lines = if sid.is_empty() {
        Vec::new()
    } else {
        transcript::all(&mut conn, &sid).await.unwrap_or_default()
    };
    let known: std::collections::HashMap<&str, &str> = lines
        .iter()
        .map(|m| (m.body_id.as_str(), m.player_id.as_str()))
        .collect();
    let bodies: Vec<String> = selected
        .iter()
        .filter(|id| known.contains_key(id.as_str()))
        .cloned()
        .collect();
    if deleting && bodies.is_empty() {
        return bad("None of those messages are in this session.");
    }

    // Pinned before anything this action appends, so the snapshot shows the
    // conversation that prompted the warning rather than the warning's own echo.
    let cut_index = if sid.is_empty() {
        0
    } else {
        transcript::len(&mut conn, &sid).await.unwrap_or(0)
    };

    let now = chrono::Utc::now().timestamp_millis();

    // Delete once for the whole action, not once per target: the moderator ticked
    // bodies, and a body has one sender regardless of how many players are being
    // warned alongside it.
    if deleting {
        let mark = transcript::DeletionMark {
            mod_user: session.username.clone(),
            mod_sub: session.sub.clone(),
            reason: reason.clone(),
            at_ms: now,
        };
        if let Err(e) = transcript::mark_deleted(&mut conn, &sid, &bodies, &mark).await {
            // Nothing has been broadcast yet, so failing here leaves the session
            // exactly as it was. Better to stop than to withdraw a message from
            // players with no record of why.
            tracing::warn!("chatmod: could not mark bodies deleted: {}", e);
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                "Could not record the deletion — nothing was removed.",
            );
        }
        for body_id in &bodies {
            state
                .signal_hub
                .broadcast(
                    &canon,
                    ServerMsg::ChatBodyDeleted {
                        body_id: body_id.clone(),
                    },
                    None,
                )
                .await;
        }
    }

    // Chat renders only in the lobby, so an in-match player has a healthy socket
    // and nowhere to show this. Checked once — the whole room shares a match.
    let in_match = state.signal_hub.match_started(&canon).await;

    let usernames = crate::api::fetch_usernames(&mut conn, &targets).await;
    let action = if deleting { "Warn + Delete" } else { "Warn" };

    let mut delivered_to = Vec::new();
    let mut missed = Vec::new();

    for player_id in &targets {
        let username = usernames.get(player_id).cloned().unwrap_or_default();

        let delivered = if in_match {
            false
        } else {
            state
                .signal_hub
                .send_to(
                    &canon,
                    player_id,
                    ServerMsg::ChatWarning {
                        reason: reason.clone(),
                    },
                )
                .await
        };

        let label = if username.is_empty() {
            player_id.clone()
        } else {
            format!("{username} ({player_id})")
        };
        if delivered {
            delivered_to.push(label);
        } else {
            missed.push(format!(
                "{label} — {}",
                if in_match { IN_MATCH } else { OFFLINE }
            ));
        }

        // The bodies this particular player sent. Any tool may cover zero, one,
        // or several, so this is always a list.
        let covered: Vec<String> = bodies
            .iter()
            .filter(|id| known.get(id.as_str()) == Some(&player_id.as_str()))
            .cloned()
            .collect();

        // The mod-side echo: the intervention sits in the conversation where it
        // happened. Never broadcast — only the target got the warning itself.
        if !sid.is_empty() {
            let echo = transcript::StoredMessage {
                body_id: crate::chat::ids::next_body_id(&mut conn)
                    .await
                    .unwrap_or_default(),
                kind: transcript::MessageKind::Warning,
                player_id: player_id.clone(),
                username: username.clone(),
                text: reason.clone(),
                flagged_words: Vec::new(),
                mod_user: session.username.clone(),
                mod_sub: session.sub.clone(),
                // A warning names no moderator on the wire, but the panel always
                // shows the real one — there is nothing to be anonymous about.
                mod_anonymous: false,
                delivered: Some(delivered),
                at_ms: now,
            };
            if let Err(e) = transcript::append(&mut conn, &canon, &echo).await {
                tracing::warn!("chatmod: warning echo not recorded: {}", e);
            }
        }

        let mut record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            action,
            &reason,
        )
        .with_target(&username, player_id.parse::<u64>().ok())
        .with_delivery(delivered);
        if !sid.is_empty() {
            record = record.with_snapshot(&sid, cut_index, covered);
        }
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Player, &record).await {
            // The action already happened. Losing the log line is bad; failing
            // the moderator's action after it took effect is worse.
            tracing::warn!("chatmod: warn audit write failed: {}", e);
        }
    }
    drop(conn);

    tracing::info!(
        session = %canon,
        moderator = %session.username,
        action = %action,
        targets = targets.len(),
        bodies = bodies.len(),
        undelivered = missed.len(),
        "chatmod: player action"
    );

    let removed = if deleting { bodies.len() } else { 0 };
    // Nobody reached is not a success, even though the deletion may have landed —
    // a green notice would read as though the warning had gone out.
    reply(
        StatusCode::OK,
        !delivered_to.is_empty(),
        warn_summary(&delivered_to, &missed, removed),
    )
}

/// Form body for the session view's Blacklist Words tool.
#[derive(serde::Deserialize)]
pub struct QuickBlacklistForm {
    #[serde(default)]
    words: String,
    #[serde(default)]
    reason: String,
}

/// POST /admin/chatmod/session/:code/blacklist
///
/// The same tool as the one on the Moderation Lists page, reachable without
/// leaving a live session. It runs the identical code path
/// ([`super::blacklist::apply_add`]) and therefore writes the identical audit
/// trail — one Word record per newly-added word, nothing for a duplicate.
///
/// Adding a word here does **not** retroactively flag messages already in the
/// transcript. `flagged_words` records what fired at broadcast time, which is
/// what players' censoring actually reflected; back-dating it would claim a
/// message was censored when every player saw it in full.
pub async fn chatmod_quick_blacklist(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Form(form): Form<QuickBlacklistForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    if chatmod_data::resolve_live_session(&state, &code)
        .await
        .is_none()
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    let words = split_words(&form.words);
    if words.is_empty() {
        return bad("Enter at least one word.");
    }

    let Ok(outcome) = super::blacklist::apply_add(&state, &session, &words, &form.reason).await
    else {
        return reply(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
            "Could not add those words.",
        );
    };

    // All-duplicate is not a success — nothing changed.
    let msg = super::blacklist::add_summary(&outcome.added, &outcome.duplicates);
    reply(StatusCode::OK, !outcome.added.is_empty(), msg)
}

/// Human summary of a warn, naming who it missed rather than reporting a smaller
/// count than the moderator targeted. Same principle as the blacklist's
/// [`super::blacklist::add_summary`]: a moderator must be able to tell a partial
/// success from a total one without reading the audit log.
pub(super) fn warn_summary(delivered: &[String], missed: &[String], removed: usize) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let deleted = match removed {
        0 => String::new(),
        n => format!(" {n} message{} removed.", plural(n)),
    };
    match (delivered.len(), missed.is_empty()) {
        (0, true) => format!("Nobody to warn.{deleted}"),
        (0, false) => format!("Warned nobody — {}.{deleted}", missed.join("; ")),
        (n, true) => format!("Warned {n} player{}.{deleted}", plural(n)),
        (n, false) => format!(
            "Warned {n} player{}.{deleted} Not delivered to {}.",
            plural(n),
            missed.join("; ")
        ),
    }
}
