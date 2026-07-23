//! The admin **Chat-Mod** tab — chat moderation (UI-layout phase).
//!
//! Every handler currently serves baked-in placeholder data so the panel's
//! layout and flow can be iterated in a browser first. Later phases replace the
//! `sample_*` functions with live data (session chat relay, SQLite-backed
//! moderation datasets, server-assigned 12-char alphanumeric body identifiers)
//! without touching the templates.
//!
//! Censoring contract for the wiring phase: when the server flags a
//! blacklisted word, the game side renders it blacked/hashed out immediately.
//! "Approve Word" un-censors the selected occurrence(s) in that chat — for
//! words that are permissible in that sentence's context — but it never
//! removes the word from the blacklist itself; future occurrences are
//! censored again and re-reviewed case by case.
//!
//! Audit-log contract for the wiring phase: every player-actionable tool
//! (Warn + Delete, Warn Only, Suspend, Ban) writes an audit record containing
//! the reason, the target's username + player id, the message body when the
//! action targeted one, and a snapshot of the chat history as it stood when
//! the action was taken. Records live in the SQLite moderation database and
//! surface in the Chat Audit Logs area of the Chat Nav. Ban additionally
//! requires an explicit confirmation dialog — a cancelled confirmation sends
//! nothing and writes no audit record.
//!
//! Scope contract: every player action binds to the player account, not the
//! session it was issued from, and governs CHAT privileges (not game access):
//! - Suspend = temporary chat mute for the entered duration — the player
//!   cannot chat in game lobbies, nor in the play field if chat moves there
//!   later, but keeps playing.
//! - Ban = permanent loss of chat privileges, lifted only by removing the
//!   player from the ban list (stored in SQLite, managed via the Moderation
//!   Lists area).
//!
//! Both apply across all sessions. Only message-body operations (delete,
//! per-occurrence word approval) are scoped to their session's chat.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use super::{require_session, templates};
use crate::state::AppState;
use templates::{ChatMessage, ChatSession, FlaggedBody, FlaggedSession, PreviewLine};

/// Shorthand for a clean (unflagged) preview line in the sample data.
fn line(text: &str) -> PreviewLine {
    PreviewLine {
        text: text.into(),
        flagged_word: None,
    }
}

/// Shorthand for a preview line carrying a blacklisted word to highlight.
fn flagged_line(text: &str, word: &str) -> PreviewLine {
    PreviewLine {
        text: text.into(),
        flagged_word: Some(word.into()),
    }
}

/// Placeholder session list for the left panel — codes and red-dot flags match
/// the design mockups (`Example Imgs/ModMainPanal.png`).
fn sample_sessions() -> Vec<ChatSession> {
    vec![
        ChatSession {
            code: "FJ5B3V".into(),
            preview: vec![
                line("Warstorm: nice shot"),
                line("PixelPirate: that portal save tho"),
                flagged_line("Warstorm: frick you all", "frick"),
            ],
            flagged: true,
        },
        ChatSession {
            code: "B874VC".into(),
            preview: vec![
                line("MossyOak: who took my ball"),
                line("TinCanTam: speed boost is up"),
                flagged_line("MossyOak: get rekt scrub", "scrub"),
            ],
            flagged: true,
        },
        ChatSession {
            code: "V5RC71".into(),
            preview: vec![
                line("Nova: ready when you are"),
                line("Quartz: one more round"),
                line("Nova: spinning up the splitter"),
            ],
            flagged: false,
        },
    ]
}

/// Placeholder flagged-message overview for the landing page. Body IDs use the
/// 12-char alphanumeric shape the server will assign once wired.
fn sample_flagged() -> Vec<FlaggedSession> {
    vec![
        FlaggedSession {
            code: "FJ5B3V".into(),
            bodies: vec![
                FlaggedBody {
                    body_id: "W34V67898701".into(),
                    username: "Warstorm".into(),
                    player_id: 7,
                    body: "frick you all".into(),
                    word: "frick".into(),
                },
                FlaggedBody {
                    body_id: "Q71ZT8C3VB55".into(),
                    username: "Warstorm".into(),
                    player_id: 7,
                    body: "no frick this whole game".into(),
                    word: "frick".into(),
                },
            ],
        },
        FlaggedSession {
            code: "B874VC".into(),
            bodies: vec![FlaggedBody {
                body_id: "K82PQ4R7M2X9".into(),
                username: "MossyOak".into(),
                player_id: 3,
                body: "get rekt scrub".into(),
                word: "scrub".into(),
            }],
        },
    ]
}

/// Placeholder transcript for the session view — a flagged exchange for
/// FJ5B3V (mirroring `Example Imgs/ModSessionEntered.png`), a clean generic
/// one for the other demo sessions so every card is enterable.
fn sample_transcript(code: &str) -> Vec<ChatMessage> {
    if code == "FJ5B3V" {
        vec![
            ChatMessage {
                body_id: "T09XB4N6QW22".into(),
                username: "PixelPirate".into(),
                player_id: 12,
                body: "that portal save tho".into(),
                flagged_word: None,
            },
            ChatMessage {
                body_id: "W34V67898701".into(),
                username: "Warstorm".into(),
                player_id: 7,
                body: "frick you all".into(),
                flagged_word: Some("frick".into()),
            },
            ChatMessage {
                body_id: "J55RD2H8PL04".into(),
                username: "PixelPirate".into(),
                player_id: 12,
                body: "chill, it is one point".into(),
                flagged_word: None,
            },
            ChatMessage {
                body_id: "Q71ZT8C3VB55".into(),
                username: "Warstorm".into(),
                player_id: 7,
                body: "no frick this whole game".into(),
                flagged_word: Some("frick".into()),
            },
        ]
    } else {
        vec![
            ChatMessage {
                body_id: "A12BC3D4E5F6".into(),
                username: "PlayerOne".into(),
                player_id: 101,
                body: "good game so far".into(),
                flagged_word: None,
            },
            ChatMessage {
                body_id: "G78HJ9K1L2M3".into(),
                username: "PlayerTwo".into(),
                player_id: 102,
                body: "watch the corner barrier".into(),
                flagged_word: None,
            },
        ]
    }
}

/// Case-insensitive lookup of a demo session; returns the canonical uppercase
/// code so links and highlights stay consistent.
fn find_session(code: &str) -> Option<String> {
    let canon = code.to_ascii_uppercase();
    sample_sessions()
        .into_iter()
        .find(|s| s.code == canon)
        .map(|s| s.code)
}

/// GET /admin/chatmod — the Chat-Mod landing page (flagged-message overview).
pub async fn chatmod_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Any authenticated role (Moderator and up) may work the moderation panel.
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };

    Html(templates::chatmod_landing_page(
        &sample_sessions(),
        &sample_flagged(),
        session.role,
        &session.username,
    ))
    .into_response()
}

/// GET /admin/chatmod/session/:code — the entered-session view. Unknown codes
/// bounce back to the landing page rather than 404ing.
pub async fn chatmod_session_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let Some(canon) = find_session(&code) else {
        return Redirect::to("/admin/chatmod").into_response();
    };

    Html(templates::chatmod_session_page(
        &canon,
        &sample_transcript(&canon),
        &sample_sessions(),
        session.role,
        &session.username,
    ))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_code_is_rejected() {
        assert!(find_session("ZZZZZZ").is_none());
    }

    #[test]
    fn known_code_is_canonicalized() {
        assert_eq!(find_session("fj5b3v").as_deref(), Some("FJ5B3V"));
    }

    #[test]
    fn landing_page_renders_flags_and_sessions() {
        let html = templates::chatmod_landing_page(
            &sample_sessions(),
            &sample_flagged(),
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(html.contains("Flagged Messages"));
        assert!(html.contains("Active Game Sessions"));
        // Sessions list + flagged list scroll inside capped containers.
        assert!(html.contains(r#"class="cm-panel-scroll""#));
        assert!(html.contains(r#"class="cm-flag-scroll""#));
        // All three demo sessions link into the session view.
        assert!(html.contains("/admin/chatmod/session/FJ5B3V"));
        assert!(html.contains("/admin/chatmod/session/V5RC71"));
        // The blacklisted word is wrapped in the highlight span.
        assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
        // Flagged cards carry sender + canonical 9-digit id + body identifier
        // on one header line, both ids tap-to-copy.
        assert!(html.contains(
            r#"Warstorm <span class="cm-pid mono" data-copy="000000007" role="button" tabindex="0" title="Copy player ID">ID 000000007</span></span><span class="cm-bodyid mono" data-copy="W34V67898701" role="button" tabindex="0" title="Copy body ID">Body ID: W34V67898701</span>"#
        ));
        // Banned List + Player Whitelist merged into one Moderation Lists nav item.
        assert!(html.contains("Moderation Lists"));
        assert!(!html.contains("Banned List"));
        assert!(!html.contains("Player Whitelist"));
    }

    #[test]
    fn session_page_renders_transcript_and_tools() {
        let html = templates::chatmod_session_page(
            "FJ5B3V",
            &sample_transcript("FJ5B3V"),
            &sample_sessions(),
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(html.contains(r#"Session Chat Code: <span class="mono cm-code">FJ5B3V</span>, You Have Entered"#));
        assert!(html.contains("Quick Access Tools"));
        assert!(html.contains("Body ID: W34V67898701"));
        assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
        // The entered session is highlighted in the left panel.
        assert!(html.contains("cm-session-active"));
        // Transcript rows use the same identity-line order as the landing
        // cards — username, then player id, then body id — both tap-to-copy.
        assert!(html.contains(
            r#"PixelPirate <span class="cm-pid mono" data-copy="000000012" role="button" tabindex="0" title="Copy player ID">ID 000000012</span> <span class="cm-bodyid mono" data-copy="T09XB4N6QW22" role="button" tabindex="0" title="Copy body ID">Body ID: T09XB4N6QW22</span>"#
        ));
        // All three panels (sessions, chat nav, tools) scroll under pinned
        // titles when content overflows their caps.
        assert_eq!(html.matches(r#"class="cm-panel-scroll""#).count(), 3);
        // Session-card previews highlight blacklisted words too, so flags in
        // OTHER sessions stay visible while a moderator is inside this one.
        assert!(html.contains(r#"MossyOak: get rekt <span class="cm-flag">scrub</span>"#));
        // Transcript flags are per-instance toggle buttons for Approve...
        assert!(html.contains(r#"class="cm-flag cm-flag-btn" data-word="frick" aria-pressed="false""#));
        // ...with the select-all-matching widener and the approve-semantics
        // hint (contextual un-censor, never un-blacklists).
        assert!(html.contains(r#"id="cm-approve-all""#));
        assert!(html.contains("Restores the word in this chat &mdash; blacklist unchanged."));
        // Blacklist accepts multiple ;-separated words; its reason is
        // audit-logged (but not player-facing).
        assert!(html.contains(r#"placeholder="Word or words &mdash; separate with ;""#));
        assert!(html.contains(r#"class="cm-reason" placeholder="Reason (logged)""#));
        // Shared optional multi-target field for Warn/Suspend/Ban (;-separated,
        // same convention as Blacklist Words), fed by the message checkboxes
        // (each carries its sender's padded id).
        assert!(html.contains(r#"id="cm-target" placeholder="Player IDs &mdash; separate with ;""#));
        assert!(html.contains(r#"data-pid="000000007""#));
        // Tools are grouped: player actions separated from the word tools.
        assert!(html.contains(r#"<p class="cm-tool-group-title">Player Actions</p>"#));
        assert!(html.contains(r#"<p class="cm-tool-group-title">Word Tools</p>"#));
        // Suspend duration is three separate fields; ≥1 required to act.
        for ph in ["Days", "Hours", "Mins"] {
            assert!(html.contains(&format!(
                r#"class="cm-dur" inputmode="numeric" placeholder="{ph}""#
            )));
        }
        assert!(html.contains("At least one duration field is required."));
        // Warn variants + suspend + ban each carry an audited, player-facing
        // reason line.
        assert!(html.contains("Warn + Delete Chat Body"));
        assert!(html.contains("Warn Only"));
        assert_eq!(
            html.matches(r#"placeholder="Reason (logged &amp; sent to player)""#)
                .count(),
            4
        );
        // Ban is guarded by a confirm/cancel dialog (accidental-click safety)
        // that spells out chat-privilege scope.
        assert!(html.contains(r#"id="cm-ban-modal""#));
        assert!(html.contains("Confirm Chat Ban"));
        assert!(html.contains("Permanently remove chat privileges for"));
        assert!(html.contains("Ban User (Chat)"));
        assert!(html.contains(r#"id="cm-ban-cancel""#));
    }
}
