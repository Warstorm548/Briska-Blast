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
//! the reason, the target's username + player id, the **set of message bodies
//! the action covered**, and a snapshot of the chat history as it stood when
//! the action was taken. Any tool — not just Warn + Delete — may act on zero,
//! one, or several of the target's bodies at once (e.g. a Warn/Suspend/Ban that
//! cites multiple messages), so the record's body list is always a `Vec`, never
//! a single id; `AuditEntry.body_ids` already reflects this. Granularity: one
//! record per **(action instance, target player)** — a single press hitting
//! several players splits into one record per player (each carrying that
//! player's covered bodies), and repeated presses on the same player are never
//! merged (a player may recur across a session). Records live in the SQLite
//! moderation database and surface in the Chat Audit Logs area of the Chat Nav.
//! Ban additionally requires an explicit confirmation dialog — a cancelled
//! confirmation sends nothing and writes no audit record.
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
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use super::{require_session, templates};
use crate::state::AppState;
use templates::{
    AuditLog, BannedUser, BlacklistWord, ChatMessage, ChatSession, FlaggedBody, FlaggedSession,
    ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
    SystemAuditEntry, WordAuditEntry,
};

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

/// A snapshot message, shorthand for the sample data below.
fn snap(body_id: &str, username: &str, player_id: u64, body: &str, word: Option<&str>) -> ChatMessage {
    ChatMessage {
        body_id: body_id.into(),
        username: username.into(),
        player_id,
        body: body.into(),
        flagged_word: word.map(Into::into),
    }
}

/// Placeholder audit records for the three Chat Audit Logs category tables.
///
/// **Player** exercises the per-(action, player) model: a multi-body Warn +
/// Delete (mirrors `Example Imgs/ChatAuditLog.png`), a bulk Ban split across two
/// players into same-timestamp rows, and EldenFire recurring across the session.
/// **Word** covers a global Blacklist (no player/body) and an Approve occurrence
/// (with sender + body + snapshot). **List** covers un-ban / lift-suspension /
/// whitelist edits (targeted at a player, no chat snapshot).
///
/// Body IDs use the 12-char alphanumeric shape the server will assign once wired.
fn sample_audit_log() -> AuditLog {
    AuditLog {
        players: vec![
            // System auto-enforcement lands in the Player log too — filled like a
            // moderator row, but Group=System and Reason names the rule that fired.
            PlayerAuditEntry {
                timestamp: "2026-07-24 14:50:00 UTC".into(),
                moderator_display: "Auto-Mod".into(),
                moderator_group: "System".into(),
                action: "Auto-Delete".into(),
                reason: "Rule: 3+ flagged words in 60s".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                body_ids: vec!["Q71ZT8C3VB55".into()],
                flagged_words: vec!["frick".into()],
                snapshot: vec![
                    snap("Q71ZT8C3VB55", "EldenFire", 12, "frick the mods", Some("frick")),
                    snap("N44QW8T1RB29", "EldenFire", 12, "you all need to hit harder", None),
                ],
            },
            PlayerAuditEntry {
                timestamp: "2026-07-24 14:47:11 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Warn Only".into(),
                reason: "Backseat modding".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                body_ids: vec![],
                flagged_words: vec![],
                snapshot: vec![
                    snap("N44QW8T1RB29", "EldenFire", 12, "you all need to hit harder", None),
                    snap("P07LM2K9XC53", "RallyKnight", 34, "we are up two, relax", None),
                ],
            },
            PlayerAuditEntry {
                timestamp: "2026-07-24 14:32:07 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Warn + Delete".into(),
                reason: "Repeat Offense".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                body_ids: vec!["T09XB4N6QW22".into(), "L88KD3F1QA72".into()],
                flagged_words: vec!["frick".into()],
                snapshot: vec![
                    snap("R21WQ7H4NM08", "RallyKnight", 34, "nice portal defense", None),
                    snap("L88KD3F1QA72", "EldenFire", 12, "frick that was my ball", Some("frick")),
                    snap("M04TC9V2HG61", "RallyKnight", 34, "easy, it is one point", None),
                    snap("T09XB4N6QW22", "EldenFire", 12, "frick this whole match", Some("frick")),
                ],
            },
            // One bulk Ban press over two players → two entries, same timestamp.
            PlayerAuditEntry {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Ban".into(),
                reason: "Slur spam".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                body_ids: vec!["Q71ZT8C3VB55".into()],
                flagged_words: vec!["frick".into()],
                snapshot: vec![
                    snap("Q71ZT8C3VB55", "EldenFire", 12, "frick the mods", Some("frick")),
                    snap("B19HN5J8WD30", "MossyOak", 3, "get rekt scrub", Some("scrub")),
                ],
            },
            PlayerAuditEntry {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Ban".into(),
                reason: "Slur spam".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                body_ids: vec!["B19HN5J8WD30".into(), "K82PQ4R7M2X9".into()],
                flagged_words: vec!["scrub".into()],
                snapshot: vec![
                    snap("B19HN5J8WD30", "MossyOak", 3, "get rekt scrub", Some("scrub")),
                    snap("K82PQ4R7M2X9", "MossyOak", 3, "scrub scrub scrub", Some("scrub")),
                ],
            },
            PlayerAuditEntry {
                timestamp: "2026-07-24 11:48:19 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Suspend 1d".into(),
                reason: "Off-topic flooding".into(),
                target_username: "TinCanTam".into(),
                target_player_id: 88,
                body_ids: vec!["H55RD2H8PL04".into()],
                flagged_words: vec![],
                snapshot: vec![snap(
                    "H55RD2H8PL04",
                    "TinCanTam",
                    88,
                    "buy my stream buy my stream buy my stream",
                    None,
                )],
            },
        ],
        words: vec![
            WordAuditEntry {
                timestamp: "2026-07-24 14:05:02 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Blacklist Word".into(),
                reason: "Slur".into(),
                word: "frick".into(),
                target_username: None,
                target_player_id: None,
                body_ids: vec![],
                snapshot: vec![],
            },
            WordAuditEntry {
                timestamp: "2026-07-24 12:19:44 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Approve Word".into(),
                reason: "Team name, not the slur".into(),
                word: "scrub".into(),
                target_username: Some("RallyKnight".into()),
                target_player_id: Some(34),
                body_ids: vec!["W71MK3P8QB20".into()],
                snapshot: vec![
                    snap("W71MK3P8QB20", "RallyKnight", 34, "gg from the scrub squad", Some("scrub")),
                    snap("Z04HD9V2LC88", "MossyOak", 3, "nice one", None),
                ],
            },
        ],
        lists: vec![
            ListAuditEntry {
                timestamp: "2026-07-24 15:10:33 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Remove Ban".into(),
                reason: "Appeal granted".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                list: "Ban List".into(),
            },
            ListAuditEntry {
                timestamp: "2026-07-24 10:02:57 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Lift Suspension".into(),
                reason: "Time served".into(),
                target_username: "TinCanTam".into(),
                target_player_id: 88,
                list: "Suspensions".into(),
            },
        ],
        system: vec![
            SystemAuditEntry {
                timestamp: "2026-07-24 14:31:55 UTC".into(),
                source: "Word Filter".into(),
                action: "Flag Word".into(),
                trigger: "Matched blacklist".into(),
                word: "frick".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                body_ids: vec!["T09XB4N6QW22".into()],
                snapshot: vec![
                    snap("R21WQ7H4NM08", "RallyKnight", 34, "nice portal defense", None),
                    snap("T09XB4N6QW22", "EldenFire", 12, "frick this whole match", Some("frick")),
                ],
            },
            SystemAuditEntry {
                timestamp: "2026-07-24 13:57:12 UTC".into(),
                source: "Word Filter".into(),
                action: "Flag Word".into(),
                trigger: "Matched blacklist".into(),
                word: "scrub".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                body_ids: vec!["K82PQ4R7M2X9".into()],
                snapshot: vec![snap(
                    "K82PQ4R7M2X9",
                    "MossyOak",
                    3,
                    "scrub scrub scrub",
                    Some("scrub"),
                )],
            },
        ],
    }
}

/// Placeholder Moderation Lists data for the three wired sub-tabs (Whitelisted
/// Users has no mockup yet). Reuses the demo players/words from the audit sample
/// so every table reads coherently against the rest of the panel.
fn sample_moderation_lists() -> ModerationLists {
    ModerationLists {
        blacklist: vec![
            BlacklistWord {
                word: "frick".into(),
                reason: "Slur".into(),
                active_filter: true,
            },
            BlacklistWord {
                word: "scrub".into(),
                reason: "Harassment".into(),
                active_filter: true,
            },
            BlacklistWord {
                word: "chicken".into(),
                reason: "Context-dependent — under review".into(),
                active_filter: false,
            },
        ],
        banned: vec![
            BannedUser {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                username: "EldenFire".into(),
                player_id: 12,
                reason: "Slur spam".into(),
                has_transcript: true,
            },
            BannedUser {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                username: "MossyOak".into(),
                player_id: 3,
                reason: "Slur spam".into(),
                has_transcript: true,
            },
        ],
        suspended: vec![SuspendedUser {
            timestamp: "2026-07-24 11:48:19 UTC".into(),
            username: "TinCanTam".into(),
            player_id: 88,
            suspended_for: "1d".into(),
            remaining: "18h 42m".into(),
            reason: "Off-topic flooding".into(),
        }],
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

/// Shared query for the Chat Nav sub-pages. `from` names the session the
/// moderator was viewing when they opened a sub-page (Moderation Lists / Chat
/// Audit Logs), so the X close can return there instead of the landing page.
#[derive(serde::Deserialize)]
pub struct FromQuery {
    from: Option<String>,
}

/// GET /admin/chatmod/audit — the Chat Audit Logs page. The X close returns to
/// the session named by `?from=` (when it resolves) or to the landing page.
pub async fn chatmod_audit_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FromQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let close_href = match query.from.as_deref().and_then(find_session) {
        Some(canon) => format!("/admin/chatmod/session/{canon}"),
        None => "/admin/chatmod".to_string(),
    };

    Html(templates::chatmod_audit_page(
        &sample_audit_log(),
        &sample_sessions(),
        &close_href,
        session.role,
        &session.username,
    ))
    .into_response()
}

/// GET /admin/chatmod/lists — the Moderation Lists page. The X close returns to
/// the session named by `?from=` (when it resolves) or to the landing page,
/// mirroring the Chat Audit Logs page's remember-where-you-were behavior.
pub async fn chatmod_lists_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FromQuery>,
) -> Response {
    let Some(session) = require_session(&headers, &state.redis).await else {
        return Redirect::to("/admin").into_response();
    };
    let close_href = match query.from.as_deref().and_then(find_session) {
        Some(canon) => format!("/admin/chatmod/session/{canon}"),
        None => "/admin/chatmod".to_string(),
    };

    Html(templates::chatmod_lists_page(
        &sample_moderation_lists(),
        &sample_sessions(),
        &close_href,
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
        // Moderation Lists is now a live Chat Nav link (not a "soon" placeholder),
        // carrying the same X-return behavior as Chat Audit Logs. The standalone
        // "Suspensions" nav entry folded into it as the Active Suspensions sub-tab.
        assert!(html.contains(r#"class="cm-nav-item cm-nav-link" href="/admin/chatmod/lists">Moderation Lists</a>"#));
        assert!(!html.contains(r#"Suspensions<span class="cm-soon">"#));
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

    #[test]
    fn audit_page_renders_table_and_snapshot_overlays() {
        let html = templates::chatmod_audit_page(
            &sample_audit_log(),
            &sample_sessions(),
            "/admin/chatmod",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(html.contains("Chat Audit Logs"));
        // A dropdown picks the category log; all four views are present.
        assert!(html.contains(r#"<select id="cm-audit-cat""#));
        for cat in ["player", "word", "list", "system"] {
            assert!(
                html.contains(&format!(r#"<div class="cm-audit-view" data-cat="{cat}""#)),
                "missing {cat} view"
            );
        }
        // Player table keeps its plain headers (Timestamp + Player ID are now
        // sortable controls, asserted separately below).
        for col in [
            "Display Name",
            "Group",
            "Action",
            "Reason",
            "Player UserName",
            "Body Id",
            "Flagged Words",
            "Transcript",
        ] {
            assert!(html.contains(&format!("<th>{col}</th>")), "missing player column {col}");
        }
        // Word/List tables bring their own direct headers.
        assert!(html.contains("<th>Word</th>"));
        assert!(html.contains("<th>List</th>"));
        // Timestamp and Player ID headers are sortable — a flip-arrow control
        // that reorders the rows (client-side).
        assert!(html.contains(
            r#"<th aria-sort="none"><button type="button" class="cm-sort" onclick="bbCmSort(this)">Timestamp<span class="cm-sort-ico" aria-hidden="true">&#9662;</span></button>"#
        ));
        assert!(html.contains(r#"class="cm-sort" onclick="bbCmSort(this)">Player ID<"#));
        // Each table's Advanced Filter has a shared spine group beside a
        // table-specific one (fieldset legends).
        assert!(html.contains("<legend>Applies to all logs</legend>"));
        assert!(html.contains("<legend>This table</legend>"));
        // The persistent spine carries a From/To time range (UTC) under the date
        // pickers, forced to a 24-hour clock via lang="en-GB".
        assert!(html.contains(r#"<label>From time (UTC)<input type="time" lang="en-GB"></label>"#));
        assert!(html.contains(r#"<label>To time (UTC)<input type="time" lang="en-GB"></label>"#));
        // Player rows: the mockup example — Warstorm/Admin warned EldenFire
        // (000000012) for a repeat offense; ids are tap-to-copy.
        assert!(html.contains("Repeat Offense"));
        assert!(html.contains(r#"data-copy="000000012""#));
        assert!(html.contains(r#"data-copy="T09XB4N6QW22""#));
        assert!(html.contains(r#"<span class="cm-flag">frick</span>"#));
        // A body-less action shows the em-dash placeholder.
        assert!(html.contains(r#"<span class="cm-audit-none">&mdash;</span>"#));
        // Two covered bodies condense into a ×N disclosure, not two rows.
        assert!(html.contains(r#"<details class="cm-audit-bodies">"#));
        assert!(html.contains("&times;2 bodies"));
        assert!(html.contains(r#"data-copy="L88KD3F1QA72""#));
        // Player Transcript opens that row's namespaced overlay; acted-on bodies
        // are tagged in the frozen snapshot.
        assert!(html.contains(r#"onclick="bbCmAuditOpen('player-0')""#));
        assert!(html.contains(r#"id="cm-audit-back-player-0""#));
        assert!(html.contains(r#"class="modal-card cm-audit-modal""#));
        assert!(html.contains(r#"<span class="cm-msg-tag">acted on</span>"#));
        // Word table: a global Blacklist (no snapshot) + an Approve occurrence
        // (index 1) which carries its own overlay.
        assert!(html.contains("Blacklist Word"));
        assert!(html.contains("Approve Word"));
        assert!(html.contains(r#"onclick="bbCmAuditOpen('word-1')""#));
        assert!(html.contains(r#"id="cm-audit-back-word-1""#));
        // List table: list-edit actions carry the list chip and no snapshot.
        assert!(html.contains("Remove Ban"));
        assert!(html.contains(r#"<span class="cm-audit-list">Ban List</span>"#));
        // System (automated) actions carry the distinct Group=System badge.
        assert!(html.contains(r#"<span class="cm-audit-sys">System</span>"#));
        // Auto-enforcement on a player lands in the PLAYER log (Group=System),
        // not the System table.
        assert!(html.contains("Auto-Delete"));
        assert!(html.contains("Auto-Mod"));
        // The System table holds flag events (non-enforcement) with overlays.
        assert!(html.contains("Flag Word"));
        assert!(html.contains("Word Filter"));
        assert!(html.contains(r#"onclick="bbCmAuditOpen('system-0')""#));
        assert!(html.contains(r#"id="cm-audit-back-system-0""#));
        // On its own page, Chat Audit Logs is the active nav item.
        assert!(html.contains("cm-nav-current"));
    }

    #[test]
    fn audit_splits_bulk_action_per_player_and_allows_recurrence() {
        let html = templates::chatmod_audit_page(
            &sample_audit_log(),
            &sample_sessions(),
            "/admin/chatmod",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        // One bulk Ban press over two players → two entries sharing a timestamp,
        // one per target player.
        assert_eq!(
            html.matches("2026-07-24 13:58:20 UTC").count(),
            2,
            "bulk ban should split into two same-timestamp rows"
        );
        assert!(html.contains("MossyOak"));
        // A player recurs across the session (EldenFire is targeted by Warn Only,
        // Warn + Delete, and Ban) — repeated actions are never merged.
        assert!(
            html.matches(r#"data-copy="000000012""#).count() >= 3,
            "EldenFire should appear as the target of several distinct actions"
        );
    }

    #[test]
    fn audit_close_target_follows_session_context() {
        // From the landing page there is no open session — the X returns there.
        let landing = templates::chatmod_audit_page(
            &sample_audit_log(),
            &sample_sessions(),
            "/admin/chatmod",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(landing.contains(r#"href="/admin/chatmod" class="cm-close""#));
        // Opened from inside a session, the X returns to that session view.
        let from_session = templates::chatmod_audit_page(
            &sample_audit_log(),
            &sample_sessions(),
            "/admin/chatmod/session/FJ5B3V",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(from_session.contains(r#"href="/admin/chatmod/session/FJ5B3V" class="cm-close""#));
    }

    #[test]
    fn lists_page_renders_subtabs_and_tables() {
        let html = templates::chatmod_lists_page(
            &sample_moderation_lists(),
            &sample_sessions(),
            "/admin/chatmod",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(html.contains("Moderation Lists"));
        // Four sub-tabs, one panel each; Backlisted Words is selected by default.
        for (tab, label) in [
            ("blacklist", "Backlisted Words"),
            ("banned", "Banned Users"),
            ("suspensions", "Active Suspensions"),
            ("whitelist", "Whitelisted Users"),
        ] {
            assert!(html.contains(&format!(r#"data-tab="{tab}""#)), "missing {tab} tab/panel");
            assert!(html.contains(label), "missing {label} label");
        }
        assert!(html.contains(
            r#"class="cm-lists-tab cm-lists-tab-active" role="tab" aria-selected="true" data-tab="blacklist""#
        ));
        // Backlisted Words: the three tools + the ledger's four columns.
        assert!(html.contains("Add to Blacklist"));
        assert!(html.contains("Remove From Blacklist"));
        assert!(html.contains("Add Words From a CSV File"));
        for col in ["Words", "Reason Provided", "Active Filter Toggle", "Delete"] {
            assert!(html.contains(&format!("<th>{col}</th>")), "missing blacklist column {col}");
        }
        // The trash button opens the inert delete-confirm modal (reason required).
        assert!(html.contains(r#"onclick="bbCmListsAsk('cm-lists-del-modal')""#));
        assert!(html.contains(r#"id="cm-lists-del-modal""#));
        // Banned Users columns + the ban/unban confirm modals.
        for col in ["Timestamp", "Username", "User ID", "Reason For Ban", "Transcript", "CheckBox"] {
            assert!(html.contains(&format!("<th>{col}</th>")), "missing banned column {col}");
        }
        assert!(html.contains(r#"onclick="bbCmListsAsk('cm-lists-ban-modal')""#));
        assert!(html.contains(r#"onclick="bbCmListsAsk('cm-lists-unban-modal')""#));
        // Banned rows show the canonical zero-padded player id, tap-to-copy.
        assert!(html.contains(r#"data-copy="000000012""#));
        // Active Suspensions columns + the three duration fields.
        for col in ["TimeStamp", "Suspended For", "Remaining Time Left"] {
            assert!(html.contains(&format!("<th>{col}</th>")), "missing suspension column {col}");
        }
        for ph in ["Days", "Hours", "Mins"] {
            assert!(html.contains(&format!(
                r#"class="cm-dur" inputmode="numeric" placeholder="{ph}""#
            )));
        }
        assert!(html.contains("Suspending a user from this page is under construction."));
        // On its own page, Moderation Lists is the active Chat Nav item.
        assert!(html.contains(r#"cm-nav-current" aria-current="page">Moderation Lists</span>"#));
    }

    #[test]
    fn lists_close_target_follows_session_context() {
        // From the landing there is no open session — the X returns there.
        let landing = templates::chatmod_lists_page(
            &sample_moderation_lists(),
            &sample_sessions(),
            "/admin/chatmod",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(landing.contains(r#"href="/admin/chatmod" class="cm-close""#));
        // Opened from inside a session, the X returns to that session view.
        let from_session = templates::chatmod_lists_page(
            &sample_moderation_lists(),
            &sample_sessions(),
            "/admin/chatmod/session/FJ5B3V",
            crate::admin::AdminRole::Moderator,
            "modtester",
        );
        assert!(from_session.contains(r#"href="/admin/chatmod/session/FJ5B3V" class="cm-close""#));
    }
}
