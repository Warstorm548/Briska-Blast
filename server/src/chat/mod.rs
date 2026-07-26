//! Chat capture and moderation data — everything behind the admin **Chat-Mod**
//! tab that isn't markup.
//!
//! The game already relays every lobby/session chat message through the server
//! (`ClientMsg::SendChat` → `ServerMsg::ChatMessage`, handled in
//! `signaling/ws/frame.rs`). This module hangs off that existing path: each
//! accepted message is given a body id ([`ids`]), matched against the blacklist
//! ([`blacklist`]), and appended to a transcript ([`transcript`]) that moderators
//! watch live. Moderation events land in [`audit`].
//!
//! # Storage
//!
//! Redis, using the same patterns as the rest of the server — plain commands, no
//! Lua round-tripping of structs, so the lua-cjson empty-array pitfall documented
//! in `api/mod.rs` does not apply here.
//!
//! **No key in this module carries a TTL.** `session:{CODE}` is the only key in
//! the deployment that expires; chat data lives and dies by explicit teardown.
//! That is a deliberate choice, and it means every path that ends a session must
//! route through [`transcript::on_session_end`] — plus the orphan sweep, for
//! sessions that vanish by passive TTL expiry with no server code running.
//!
//! # Retention
//!
//! A transcript is written from a session's first message so a moderator has
//! something live to watch, but survives only when something makes it evidence:
//!
//! | Trigger | Outcome |
//! |---|---|
//! | Session ends clean | transcript deleted outright, unrecoverable |
//! | Blacklisted word detected | retained permanently + a System audit record |
//! | Moderator acts, or types a single message | retained permanently |
//!
//! Retention is retroactive: it keeps the *whole* conversation, not just the part
//! after the trigger, which is the point of capturing from the first message.
//!
//! A "snapshot of the chat as it stood" is not a copy. The transcript is stored
//! once and each audit record pins a cut index plus the body ids it covered;
//! rendering replays the transcript up to that cut. This is only sound because
//! the transcript is append-only — a future Delete Body must write a tombstone
//! rather than removing the entry, or older snapshots stop being faithful.

pub mod audit;
pub mod blacklist;
pub mod ids;
pub mod transcript;

use crate::signaling::protocol::{ChatKind, ServerMsg};
use crate::state::AppState;
use transcript::{MessageKind, StoredMessage};

/// A moderator speaking into a live session.
pub struct ModeratorMessage<'a> {
    /// The name players will see — either the moderator's Pocket ID display name
    /// or the generic anonymous label.
    pub display_name: &'a str,
    /// The moderator's real display name. Recorded regardless of `anonymous`.
    pub moderator: &'a str,
    /// The moderator's Pocket ID subject — stable across a display-name change,
    /// which is why the record keeps both.
    pub moderator_sub: &'a str,
    pub anonymous: bool,
    pub text: &'a str,
}

/// Post a moderator message into a live session's chat.
///
/// Two things are deliberately asymmetric here:
///
/// 1. **The broadcast may be anonymous; the record never is.** The transcript
///    stores the acting moderator's real name and subject alongside the
///    `anonymous` flag, so an anonymous intervention is still attributable.
///    Only `display_name` reaches players.
/// 2. **Moderator text is not run through the word filter.** A moderator
///    quoting a slur in order to moderate it must not have their own message
///    censored, and they are trusted staff by definition.
///
/// Appending marks the transcript retained (see [`transcript::append`]), so a
/// deliberate intervention in a player-facing channel stays on the record even
/// if the session would otherwise have been discarded.
///
/// Broadcasting works from an admin handler because both routers share one
/// `AppState`, and therefore one `SignalHub`.
pub async fn speak_as_moderator(state: &AppState, code: &str, msg: ModeratorMessage<'_>) {
    match state.redis.get().await {
        Ok(mut conn) => match ids::next_body_id(&mut conn).await {
            Ok(body_id) => {
                let stored = StoredMessage {
                    body_id,
                    kind: MessageKind::Moderator,
                    // A moderator has no player account.
                    player_id: String::new(),
                    username: msg.display_name.to_string(),
                    text: msg.text.to_string(),
                    flagged_words: Vec::new(),
                    mod_user: msg.moderator.to_string(),
                    mod_sub: msg.moderator_sub.to_string(),
                    mod_anonymous: msg.anonymous,
                    at_ms: chrono::Utc::now().timestamp_millis(),
                };
                if let Err(e) = transcript::append(&mut conn, code, &stored).await {
                    tracing::warn!("chat: moderator line not recorded for {}: {}", code, e);
                }
            }
            Err(e) => tracing::warn!("chat: no body id for the moderator line: {}", e),
        },
        Err(e) => tracing::warn!("chat: moderator line not recorded, no redis: {}", e),
    }

    // Send it either way. A recording failure must not stop a moderator being
    // heard — the same "never silence the channel" rule the player path follows.
    state
        .signal_hub
        .broadcast(
            code,
            ServerMsg::ChatMessage {
                from: String::new(),
                username: msg.display_name.to_string(),
                text: msg.text.to_string(),
                kind: ChatKind::Moderator,
            },
            None,
        )
        .await;

    tracing::info!(
        session = %code,
        moderator = %msg.moderator,
        anonymous = msg.anonymous,
        "chat: moderator spoke into session"
    );
}

/// Record an accepted player chat line and resolve the sender's display name.
///
/// Returns the display name to broadcast, empty when it can't be resolved (the
/// client then falls back to `Player <id>`, the pre-existing behaviour).
///
/// `text` must be the **original**, uncensored body — masking happens at the
/// broadcast, not here, because the transcript is what a moderator judges.
///
/// Every failure degrades to "the message still goes out". A moderation outage
/// must never silence the game, so a Redis error costs the audit trail for that
/// line, not the line itself. This mirrors how the username lookup already
/// degraded before capture existed.
///
/// One pool checkout covers the username lookup, the id mint, the append, and
/// the audit write — they were separate round trips otherwise.
pub async fn capture_player_message(
    state: &AppState,
    code: &str,
    player_id: &str,
    text: &str,
    flagged_words: Vec<String>,
) -> String {
    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("chat: capture skipped for {}, no redis: {}", code, e);
            return String::new();
        }
    };

    // Re-read the name every message so a mid-session rename is reflected.
    let username = crate::api::fetch_usernames(&mut conn, &[player_id.to_string()])
        .await
        .remove(player_id)
        .unwrap_or_default();

    let body_id = match ids::next_body_id(&mut conn).await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("chat: could not mint a body id: {}", e);
            return username;
        }
    };

    let flagged = !flagged_words.is_empty();
    let message = StoredMessage {
        body_id: body_id.clone(),
        kind: MessageKind::Player,
        player_id: player_id.to_string(),
        username: username.clone(),
        text: text.to_string(),
        flagged_words: flagged_words.clone(),
        mod_user: String::new(),
        mod_sub: String::new(),
        mod_anonymous: false,
        at_ms: chrono::Utc::now().timestamp_millis(),
    };

    let sid = match transcript::append(&mut conn, code, &message).await {
        Ok(sid) => sid,
        Err(e) => {
            tracing::warn!("chat: could not append to the transcript: {}", e);
            return username;
        }
    };

    if flagged {
        // The flag itself is a System record — an automated, non-enforcement
        // event. Auto-*enforcement* would instead land in the Player table with
        // group = System; nothing enforces automatically yet.
        let cut_index = transcript::len(&mut conn, &sid).await.unwrap_or(0);
        let record = audit::AuditRecord::by_system(
            audit::WORD_FILTER_SOURCE,
            "Flag Word",
            "Matched blacklist",
        )
        .with_target(&username, player_id.parse::<u64>().ok())
        .with_words(flagged_words)
        .with_snapshot(&sid, cut_index, vec![body_id]);

        if let Err(e) = audit::write(&mut conn, audit::AuditCategory::System, &record).await {
            tracing::warn!("chat: could not write the flag audit record: {}", e);
        }
    }

    username
}
