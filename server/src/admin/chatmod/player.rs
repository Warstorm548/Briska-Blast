//! Player moderation tools: **Warn Only**, **Warn + Delete Chat Body**, and
//! **Ban User (Chat)**.
//!
//! All three post from the session view and answer with JSON rather than a
//! redirect, because that page polls its transcript every two seconds —
//! navigating away and back would throw away the moderator's place in a live
//! conversation. That is the only reason these differ from the Moderation Lists
//! tools.
//!
//! They also share one code path ([`apply`]) rather than one per button. The
//! target resolution, the transcript echo and the audit write are identical
//! between them, and two copies of an audit path drift — the same reason
//! [`super::blacklist::apply_add`] is shared by its two callers.
//!
//! # What a warning is
//!
//! A one-off notice to a single player, carrying the reason the moderator typed.
//! It is not a privilege change: unlike Suspend and Ban, nothing about the
//! player's account changes, so there is no state to lift later.
//!
//! Delivery is **live-only and never queued**. A player who has disconnected
//! simply does not receive it; the attempt is recorded as undelivered on both the
//! audit record and the transcript echo, and the moderator is told. Holding
//! warnings for later would deliver them detached from the conversation that
//! prompted them.
//!
//! Being in a match is no longer a barrier — chat renders there too, so a
//! connected player is a reachable one wherever they are.
//!
//! # What a ban is
//!
//! A permanent, account-wide loss of chat privileges, lifted only from the
//! Moderation Lists page. The player keeps playing — a chat ban never touches
//! game access. Enforcement lives in `chat::bans`, not here; this module only
//! writes the entry.
//!
//! Its notice is delivered on the same best-effort terms as a warning, but the
//! ban does not depend on it: [`crate::chat::bans`] re-sends the notice on every
//! refused message, so a player who was offline finds out the moment they try to
//! speak.
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
use deadpool_redis::redis::AsyncCommands;

use super::super::{chatmod_data, require_session};
use super::blacklist::split_words;
use crate::admin::AdminSession;
use crate::chat::{audit, bans, transcript, MAX_CHAT_LEN};
use crate::signaling::protocol::ServerMsg;
use crate::state::AppState;

/// Form body shared by all three player buttons. `targets` and `body_ids` are
/// `;`-separated, matching the separator convention the panel uses everywhere.
/// `delete` is `1` for Warn + Delete Chat Body and absent otherwise; the ban
/// button never sets it.
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

/// Which tool was pressed. The differences between them are small enough to
/// thread through one path, and keeping them together is what stops the audit
/// trail diverging per button.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    /// Warn Only, or Warn + Delete Chat Body when the flag is set.
    Warn { delete: bool },
    Ban,
}

impl Action {
    /// The `action` string on the audit record. These match the vocabulary the
    /// Chat Audit Logs filter panel already offers, so a moderator filtering for
    /// "Ban" finds these rows.
    fn label(self) -> &'static str {
        match self {
            Self::Warn { delete: true } => "Warn + Delete",
            Self::Warn { delete: false } => "Warn",
            Self::Ban => "Ban",
        }
    }

    fn transcript_kind(self) -> transcript::MessageKind {
        match self {
            Self::Warn { .. } => transcript::MessageKind::Warning,
            Self::Ban => transcript::MessageKind::Ban,
        }
    }

    fn frame(self, reason: String) -> ServerMsg {
        match self {
            Self::Warn { .. } => ServerMsg::ChatWarning { reason },
            Self::Ban => ServerMsg::ChatBanned { reason },
        }
    }

    fn deletes(self) -> bool {
        matches!(self, Self::Warn { delete: true })
    }
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

/// Why a notice did not reach someone. Phrased for a moderator reading a notice.
const OFFLINE: &str = "not connected";
const NOT_HERE: &str = "not in this session";

/// POST /admin/chatmod/session/:code/warn
pub async fn chatmod_warn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Form(form): Form<WarnForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let delete = form.delete == "1";
    apply(&state, &session, &code, &form, Action::Warn { delete }).await
}

/// POST /admin/chatmod/session/:code/ban
///
/// Bans every named target from chat, permanently and account-wide. The reason
/// reaches the player; lifting it is a Moderation Lists action.
pub async fn chatmod_ban(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Form(form): Form<WarnForm>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    apply(&state, &session, &code, &form, Action::Ban).await
}

/// The shared body of every player tool.
///
/// One audit record per **(action instance, target player)** — a press hitting
/// three players writes three records, each carrying the bodies that player sent.
/// Repeated presses are never merged: a player may be warned several times in one
/// session and each is its own row.
async fn apply(
    state: &AppState,
    session: &AdminSession,
    code: &str,
    form: &WarnForm,
    action: Action,
) -> Response {
    let Some(canon) = chatmod_data::resolve_live_session(state, code).await else {
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
    let deleting = action.deletes();
    if deleting && selected.is_empty() {
        return bad("Tick at least one message to delete, or use Warn Only.");
    }

    let Ok(mut conn) = state.redis.get().await else {
        return reply(StatusCode::SERVICE_UNAVAILABLE, false, "Storage unavailable.");
    };

    // No transcript means no chat happened here: there is nothing to delete and
    // no conversation to pin a snapshot to. A warning or ban still applies.
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

    // A target must have something to do with this session: seated now (which
    // covers a player inside their reconnect grace window), or on record as
    // having spoken. Checked before anything is written.
    //
    // Without this, one typo in a `;`-separated field writes a permanent audit
    // record — and a transcript echo naming them — against a player who was
    // never in the conversation. For a ban it would also write a permanent chat
    // ban against a stranger. Manual deletion of audit records is not built, so
    // that record would be unremovable.
    //
    // Deliberately *not* "currently connected": acting on someone who just said
    // something and quit is the common case, and it must still land in the log
    // as undelivered.
    let seated: Option<crate::api::Session> = conn
        .get::<_, Option<String>>(format!("session:{canon}"))
        .await
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok());
    let spoke: std::collections::HashSet<&str> =
        lines.iter().map(|m| m.player_id.as_str()).collect();
    let (targets, strangers): (Vec<String>, Vec<String>) = targets.into_iter().partition(|id| {
        seated.as_ref().is_some_and(|s| s.contains_player(id)) || spoke.contains(id.as_str())
    });
    if targets.is_empty() {
        return bad(format!(
            "No valid target — {} not in this session.",
            strangers.join(", ")
        ));
    }

    // Pinned before anything this action appends, so a warning's snapshot shows
    // the conversation that prompted it rather than its own echo. For a ban the
    // whole transcript renders and this marks where the ban fell.
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

    let usernames = crate::api::fetch_usernames(&mut conn, &targets).await;

    // Kept as separate lists rather than one pre-formatted "missed" pile: the
    // reply has to distinguish "acted on but the notice missed" from "not acted
    // on at all", and re-reading that distinction out of display strings would
    // break the moment the wording changed.
    let mut delivered_to = Vec::new();
    let mut undelivered: Vec<String> = Vec::new();
    // Strangers are reported but never acted on — no send, no echo, no record.
    let strangers_note: Vec<String> = strangers
        .iter()
        .map(|id| format!("{id} — {NOT_HERE}"))
        .collect();
    // Already banned: the player is in the state the moderator wanted, so this is
    // not a failure, but no second record is written.
    let mut already: Vec<String> = Vec::new();
    // Targets a ban could not be written for. Reported separately from a failed
    // delivery: the notice not landing is expected and harmless, the ban itself
    // not being recorded means the player is not actually banned.
    let mut failed: Vec<String> = Vec::new();

    for player_id in &targets {
        let username = usernames.get(player_id).cloned().unwrap_or_default();
        let label = if username.is_empty() {
            player_id.clone()
        } else {
            format!("{username} ({player_id})")
        };

        // The bodies this particular player sent. Any tool may cover zero, one,
        // or several, so this is always a list.
        let covered: Vec<String> = bodies
            .iter()
            .filter(|id| known.get(id.as_str()) == Some(&player_id.as_str()))
            .cloned()
            .collect();
        // The blacklisted words that fired on those bodies — what the audit
        // table renders as red chips. Read from the transcript rather than the
        // form for the same reason the sender map is: the server knows what
        // actually fired at broadcast time.
        let words: Vec<String> = lines
            .iter()
            .filter(|m| covered.iter().any(|id| id == &m.body_id))
            .flat_map(|m| m.flagged_words.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        // The ban is written before the notice goes out: if the entry fails, the
        // player must not be told they are banned when they are not.
        if action == Action::Ban {
            let entry = bans::BanEntry {
                player_id: bans::numeric_id(player_id).unwrap_or_default(),
                username: username.clone(),
                reason: reason.clone(),
                words: words.clone(),
                banned_by: session.username.clone(),
                banned_sub: session.sub.clone(),
                at_ms: now,
                sid: sid.clone(),
                cut_index,
            };
            match bans::ban(&mut conn, std::slice::from_ref(player_id), &entry).await {
                // A second ban writes no second record: the first one already
                // holds the reason and the evidence, and overwriting it would
                // leave the list disagreeing with the audit log.
                Ok(outcome) if outcome.banned.is_empty() => {
                    already.push(label);
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("chatmod: ban write failed for {}: {}", player_id, e);
                    failed.push(label);
                    continue;
                }
            }
        }

        // A live socket is the whole test: chat renders in the match as well as
        // the lobby, so there is no longer a state in which a connected player
        // has nowhere to show this.
        let delivered = state
            .signal_hub
            .send_to(&canon, player_id, action.frame(reason.clone()))
            .await;

        if delivered {
            delivered_to.push(label);
        } else {
            undelivered.push(format!("{label} — {OFFLINE}"));
        }

        // The mod-side echo: the intervention sits in the conversation where it
        // happened. Never broadcast — only the target got the notice itself.
        if !sid.is_empty() {
            let echo = transcript::StoredMessage {
                body_id: crate::chat::ids::next_body_id(&mut conn)
                    .await
                    .unwrap_or_default(),
                kind: action.transcript_kind(),
                player_id: player_id.clone(),
                username: username.clone(),
                text: reason.clone(),
                flagged_words: Vec::new(),
                mod_user: session.username.clone(),
                mod_sub: session.sub.clone(),
                // A warning or ban names no moderator on the wire, but the panel
                // always shows the real one — there is nothing to be anonymous
                // about.
                mod_anonymous: false,
                delivered: Some(delivered),
                at_ms: now,
            };
            if let Err(e) = transcript::append(&mut conn, &canon, &echo).await {
                tracing::warn!("chatmod: action echo not recorded: {}", e);
            }
        }

        let mut record = audit::AuditRecord::by_moderator(
            &session.username,
            &session.sub,
            session.role,
            action.label(),
            &reason,
        )
        .with_target(&username, player_id.parse::<u64>().ok())
        .with_words(words)
        .with_delivery(delivered);
        // A ban also edits the ban list, so the same record is tagged to appear
        // in the List table. A warning edits no list and stays out of it.
        if action == Action::Ban {
            record = record.with_list(crate::chat::bans::AUDIT_LIST_NAME);
        }
        if !sid.is_empty() {
            record = record.with_snapshot(&sid, cut_index, covered);
            // A ban keeps the whole conversation, not just what preceded it —
            // it is permanent in a way the other tools are not, so the record is
            // expected to answer what else happened here.
            if action == Action::Ban {
                record = record.full();
            }
        }
        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::Player, &record).await {
            // The action already happened. Losing the log line is bad; failing
            // the moderator's action after it took effect is worse.
            tracing::warn!("chatmod: player action audit write failed: {}", e);
        }
    }
    drop(conn);

    tracing::info!(
        session = %canon,
        moderator = %session.username,
        action = %action.label(),
        targets = targets.len(),
        bodies = bodies.len(),
        undelivered = undelivered.len(),
        "chatmod: player action"
    );

    match action {
        Action::Warn { .. } => {
            // A warn reports strangers and undelivered notices the same way —
            // both are people the warning did not reach.
            let missed: Vec<String> = strangers_note
                .into_iter()
                .chain(undelivered)
                .collect();
            let removed = if deleting { bodies.len() } else { 0 };
            // Nobody reached is not a success, even though the deletion may have
            // landed — a green notice would read as though the warning had gone
            // out.
            reply(
                StatusCode::OK,
                !delivered_to.is_empty(),
                warn_summary(&delivered_to, &missed, removed),
            )
        }
        Action::Ban => {
            // A ban applies whether or not its notice arrived, so success is
            // "somebody is now banned", not "somebody was told". Reporting an
            // applied ban as a failure would invite a moderator to press again.
            let applied = !delivered_to.is_empty() || !undelivered.is_empty();
            // A failed target is someone the moderator selected who is still not
            // banned. That must not read as success on the strength of *other*
            // targets having already been banned before this press — green beside
            // "could not ban: X" invites the moderator to move on.
            let ok = if failed.is_empty() {
                applied || !already.is_empty()
            } else {
                applied
            };
            reply(
                StatusCode::OK,
                ok,
                ban_summary(
                    delivered_to.len() + undelivered.len(),
                    &undelivered,
                    &already,
                    &strangers_note,
                    &failed,
                ),
            )
        }
    }
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

    let outcome = match super::blacklist::apply_add(&state, &session, &words, &form.reason).await {
        Ok(outcome) => outcome,
        Err(super::blacklist::AddError::NoStorage) => {
            return reply(StatusCode::SERVICE_UNAVAILABLE, false, "Storage unavailable.")
        }
        Err(super::blacklist::AddError::AddFailed) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                false,
                "Could not add those words.",
            )
        }
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

/// Human summary of a ban.
///
/// Deliberately shaped differently from [`warn_summary`]. A warning that reaches
/// nobody has done nothing, so delivery is its headline; a ban applies whether or
/// not its notice arrived, so the headline is how many players are now banned and
/// an undelivered notice is a footnote. `failed` is the case that actually
/// matters — those players are *not* banned, despite the moderator pressing the
/// button.
///
/// `banned` counts players newly written to the list. `already` are those the
/// tool found were banned before, `strangers` were never in the session and were
/// not acted on at all.
pub(super) fn ban_summary(
    banned: usize,
    undelivered: &[String],
    already: &[String],
    strangers: &[String],
    failed: &[String],
) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let mut msg = match banned {
        0 => "Banned nobody.".to_string(),
        n => format!("Banned {n} player{} from chat.", plural(n)),
    };
    if !already.is_empty() {
        msg.push_str(&format!(" Already banned: {}.", already.join("; ")));
    }
    if !undelivered.is_empty() {
        // The ban still applies — they are told the next time they try to speak.
        msg.push_str(&format!(
            " Notice not delivered to {}.",
            undelivered.join("; ")
        ));
    }
    if !strangers.is_empty() {
        msg.push_str(&format!(" Skipped {}.", strangers.join("; ")));
    }
    if !failed.is_empty() {
        // The one line a moderator must not skim past.
        msg.push_str(&format!(
            " NOT banned, could not be recorded: {}.",
            failed.join("; ")
        ));
    }
    msg
}
