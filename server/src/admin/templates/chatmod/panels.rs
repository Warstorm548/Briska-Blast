//! The scrolling card/row lists the shell and pages inject: the left session
//! panel, the landing page's flagged cards, and the session transcript. Each is
//! also served on its own by the live-refresh endpoints, which is what the
//! `chatmod_*_fragment` wrappers at the bottom are for.

use shared::types::player::PlayerId;

use super::super::common::escape;
use super::highlight::{highlight, highlight_toggle};
use super::model::{message_role, ChatMessage, ChatSession, FlaggedSession, MessageRole};

/// The left "Active Game Sessions" card list. `active_code` highlights the
/// session currently entered (session view only).
pub(super) fn session_list_html(sessions: &[ChatSession], active_code: Option<&str>) -> String {
    if sessions.is_empty() {
        return r#"<p class="cm-empty">No active sessions.</p>"#.to_string();
    }
    sessions
        .iter()
        .map(|s| {
            let code = escape(&s.code);
            let active = if active_code == Some(s.code.as_str()) {
                " cm-session-active"
            } else {
                ""
            };
            let dot = if s.flagged {
                r#"<span class="cm-dot" title="Has flagged messages"></span>"#
            } else {
                ""
            };
            let preview = s
                .preview
                .iter()
                .map(|line| match &line.flagged_word {
                    Some(word) => highlight(&line.text, word),
                    None => escape(&line.text),
                })
                .collect::<Vec<_>>()
                .join("<br>");
            format!(
                r#"<a href="/admin/chatmod/session/{code}" class="cm-session-card{active}">{dot}
          <div class="cm-session-code">code: {code}</div>
          <div class="cm-session-preview">{preview}</div>
        </a>"#
            )
        })
        .collect()
}

/// The landing page's flagged-message cards — one per session with flags, each
/// linking into that session's view.
pub(super) fn flagged_list_html(flagged: &[FlaggedSession]) -> String {
    if flagged.is_empty() {
        return r#"<p class="cm-empty">No flagged messages.</p>"#.to_string();
    }
    flagged
        .iter()
        .map(|f| {
            let code = escape(&f.code);
            let bodies = f
                .bodies
                .iter()
                .map(|b| {
                    format!(
                        r#"<div class="cm-flag-user"><span>{user} <span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span></span><span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></div>
          <div class="cm-flag-body">{body}</div>"#,
                        user = escape(&b.username),
                        pid = PlayerId::from_counter(b.player_id),
                        body = highlight(&b.body, &b.word),
                        id = escape(&b.body_id),
                    )
                })
                .collect::<String>();
            format!(
                r#"<a href="/admin/chatmod/session/{code}" class="cm-flag-card">
          <div class="cm-flag-code">Code: {code}</div>
          {bodies}
        </a>"#
            )
        })
        .collect()
}

/// The " (as “Mod”)" suffix on an anonymously-posted moderator line.
///
/// The transcript names the real moderator; this records how the line appeared
/// to players. With several moderators in one session, two anonymous posts would
/// otherwise be indistinguishable from each other on the moderation surface too,
/// which is not what the anonymity toggle is for.
pub(super) fn posted_as_html(m: &ChatMessage) -> String {
    match &m.posted_as {
        Some(shown) => format!(
            r#" <span class="cm-msg-as">as &ldquo;{}&rdquo;</span>"#,
            escape(shown)
        ),
        None => String::new(),
    }
}

/// The session view's transcript rows: body identifier + username header (with
/// a select checkbox for the tools panel) above the message text.
pub(super) fn transcript_html(transcript: &[ChatMessage]) -> String {
    if transcript.is_empty() {
        return r#"<p class="cm-empty">No messages in this session yet.</p>"#.to_string();
    }
    transcript
        .iter()
        .map(|m| {
            let body = match &m.flagged_word {
                Some(word) => highlight_toggle(&m.body, word),
                None => escape(&m.body),
            };
            let id = escape(&m.body_id);
            let role = message_role(m);
            // Neither a moderator line nor a warning takes a select checkbox. The
            // player tools act on what a player said, and a moderator is not a
            // player; a warning's player id names its *target*, so offering it as
            // a selectable body would let a moderator act on their own message as
            // though the player had sent it.
            let selectable = role == MessageRole::Player;
            let (pid_chip, select) = match m.player_id {
                Some(pid) => {
                    let pid = PlayerId::from_counter(pid);
                    let chip = format!(
                        r#"<span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span> "#
                    );
                    let select = if selectable {
                        format!(r#"<input type="checkbox" data-pid="{pid}" data-body="{id}" aria-label="Select message {id}">"#)
                    } else {
                        String::new()
                    };
                    (chip, select)
                }
                None => (String::new(), String::new()),
            };
            let tag = match role {
                MessageRole::Player => "",
                MessageRole::Moderator => r#"<span class="cm-msg-mod">MOD</span> "#,
                MessageRole::Warning => r#"<span class="cm-msg-warn">WARNED</span> "#,
                MessageRole::Ban => r#"<span class="cm-msg-ban">BANNED</span> "#,
            };
            // A warning's header reads as an action on someone, not as something
            // they said — and says plainly when it never reached them, because
            // warnings are not queued and a moderator would otherwise assume it
            // landed.
            let sent_to = match (role, m.delivered) {
                (MessageRole::Warning | MessageRole::Ban, Some(true)) => {
                    r#"<span class="cm-msg-sent">sent to player</span> "#.to_string()
                }
                (MessageRole::Warning | MessageRole::Ban, _) => {
                    r#"<span class="cm-msg-undelivered">not delivered</span> "#.to_string()
                }
                _ => String::new(),
            };
            let (state_class, notice) = match &m.deleted {
                Some(d) => (
                    " cm-msg-deleted",
                    format!(
                        r#"<p class="cm-msg-removed">Removed from players&rsquo; view by {who} &middot; {at}{why}</p>"#,
                        who = escape(&d.mod_user),
                        at = escape(&d.at),
                        why = if d.reason.is_empty() {
                            String::new()
                        } else {
                            format!(" &middot; {}", escape(&d.reason))
                        },
                    ),
                ),
                None => ("", String::new()),
            };
            let role_class = match role {
                MessageRole::Warning => " cm-msg-warning",
                MessageRole::Ban => " cm-msg-banned",
                _ => "",
            };
            format!(
                r#"<div class="cm-msg{role_class}{state_class}">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{tag}{sent_to}{user}{posted_as} {pid_chip}<span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></span>
              {select}
            </div>
            {notice}<div class="cm-msg-body">{body}</div>
          </div>"#,
                user = escape(&m.username),
                posted_as = posted_as_html(m),
            )
        })
        .collect()
}

/// The left panel's card list, for the live-refresh endpoint. Renders exactly
/// what the full page puts inside `#cm-sessions`, so a poll can swap it in.
pub fn chatmod_sessions_fragment(sessions: &[ChatSession], active_code: Option<&str>) -> String {
    session_list_html(sessions, active_code)
}

/// The landing page's flagged cards, for the live-refresh endpoint.
pub fn chatmod_flagged_fragment(flagged: &[FlaggedSession]) -> String {
    flagged_list_html(flagged)
}

/// A session's transcript rows, for the live-refresh endpoint.
pub fn chatmod_transcript_fragment(transcript: &[ChatMessage]) -> String {
    transcript_html(transcript)
}
