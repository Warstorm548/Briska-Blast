//! The scrolling card/row lists the shell and pages inject: the left session
//! panel, the landing page's flagged cards, and the session transcript. Each is
//! also served on its own by the live-refresh endpoints, which is what the
//! `chatmod_*_fragment` wrappers at the bottom are for.

use shared::types::player::PlayerId;

use super::super::common::escape;
use super::highlight::{highlight, highlight_toggle};
use super::model::{ChatMessage, ChatSession, FlaggedSession};

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
            // A moderator line carries no player id and no select checkbox —
            // the player tools act on player accounts, and a moderator is not
            // one. The MOD tag keeps it from reading as a player's message.
            let (pid_chip, select) = match m.player_id {
                Some(pid) => {
                    let pid = PlayerId::from_counter(pid);
                    (
                        format!(
                            r#"<span class="cm-pid mono" data-copy="{pid}" role="button" tabindex="0" title="Copy player ID">ID {pid}</span> "#
                        ),
                        format!(r#"<input type="checkbox" data-pid="{pid}" aria-label="Select message {id}">"#),
                    )
                }
                None => (String::new(), String::new()),
            };
            let mod_tag = if m.is_moderator {
                r#"<span class="cm-msg-mod">MOD</span> "#
            } else {
                ""
            };
            format!(
                r#"<div class="cm-msg">
            <div class="cm-msg-head">
              <span class="cm-msg-user">{mod_tag}{user}{posted_as} {pid_chip}<span class="cm-bodyid mono" data-copy="{id}" role="button" tabindex="0" title="Copy body ID">Body ID: {id}</span></span>
              {select}
            </div>
            <div class="cm-msg-body">{body}</div>
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
