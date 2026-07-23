//! The admin **Chat-Mod** tab — chat moderation (UI-layout phase).
//!
//! Every handler currently serves baked-in placeholder data so the panel's
//! layout and flow can be iterated in a browser first. Later phases replace the
//! `sample_*` functions with live data (session chat relay, SQLite-backed
//! moderation datasets, server-assigned 12-char alphanumeric body identifiers)
//! without touching the templates.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use super::{require_session, templates};
use crate::state::AppState;
use templates::{ChatMessage, ChatSession, FlaggedBody, FlaggedSession};

/// Placeholder session list for the left panel — codes and red-dot flags match
/// the design mockups (`Example Imgs/ModMainPanal.png`).
fn sample_sessions() -> Vec<ChatSession> {
    vec![
        ChatSession {
            code: "FJ5B3V".into(),
            preview: vec![
                "Warstorm: nice shot".into(),
                "PixelPirate: that portal save tho".into(),
                "Warstorm: frick you all".into(),
            ],
            flagged: true,
        },
        ChatSession {
            code: "B874VC".into(),
            preview: vec![
                "MossyOak: who took my ball".into(),
                "TinCanTam: speed boost is up".into(),
                "MossyOak: get rekt scrub".into(),
            ],
            flagged: true,
        },
        ChatSession {
            code: "V5RC71".into(),
            preview: vec![
                "Nova: ready when you are".into(),
                "Quartz: one more round".into(),
                "Nova: spinning up the splitter".into(),
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
            bodies: vec![FlaggedBody {
                body_id: "W34V67898701".into(),
                body: "frick you all".into(),
                word: "frick".into(),
            }],
        },
        FlaggedSession {
            code: "B874VC".into(),
            bodies: vec![FlaggedBody {
                body_id: "K82PQ4R7M2X9".into(),
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
                body: "that portal save tho".into(),
                flagged_word: None,
            },
            ChatMessage {
                body_id: "W34V67898701".into(),
                username: "Warstorm".into(),
                body: "frick you all".into(),
                flagged_word: Some("frick".into()),
            },
            ChatMessage {
                body_id: "J55RD2H8PL04".into(),
                username: "PixelPirate".into(),
                body: "chill, it is one point".into(),
                flagged_word: None,
            },
        ]
    } else {
        vec![
            ChatMessage {
                body_id: "A12BC3D4E5F6".into(),
                username: "PlayerOne".into(),
                body: "good game so far".into(),
                flagged_word: None,
            },
            ChatMessage {
                body_id: "G78HJ9K1L2M3".into(),
                username: "PlayerTwo".into(),
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
        // All three demo sessions link into the session view.
        assert!(html.contains("/admin/chatmod/session/FJ5B3V"));
        assert!(html.contains("/admin/chatmod/session/V5RC71"));
        // The blacklisted word is wrapped in the highlight span.
        assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
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
        assert!(html.contains("Session Chat Code: FJ5B3V, You Have Entered"));
        assert!(html.contains("Quick Access Tools"));
        assert!(html.contains("Body ID: W34V67898701"));
        assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
        // The entered session is highlighted in the left panel.
        assert!(html.contains("cm-session-active"));
    }
}
