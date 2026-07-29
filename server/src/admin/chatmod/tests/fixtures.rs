//! Sample data shared by the Chat-Mod tests.

use crate::admin::templates::{
    AuditLog, BannedUser, BlacklistWord, ChatMessage, ChatSession, FlaggedBody, FlaggedSession,
    ListAuditEntry, ModerationLists, PlayerAuditEntry, PreviewLine, SuspendedUser,
    SystemAuditEntry, WordAuditEntry,
};

// ---------------------------------------------------------------------
// Rendering fixtures.
//
// These were the panel's placeholder data while the UI was built against
// mock content; the handlers now read live Redis instead. They are kept
// here, and only here, because they exercise every branch the templates
// can take — flagged and clean previews, a multi-body `xN` disclosure, a
// body-less action's em-dash, a bulk action split across two players, and
// the Group=System badge. Driving the template tests from live Redis would
// need a running server and would cover far less.
// ---------------------------------------------------------------------

/// Fixture shorthand: for a clean (unflagged) preview line in the sample data.
pub(super) fn line(text: &str) -> PreviewLine {
    PreviewLine {
        text: text.into(),
        flagged_word: None,
    }
}

/// Fixture shorthand: for a preview line carrying a blacklisted word to highlight.
pub(super) fn flagged_line(text: &str, word: &str) -> PreviewLine {
    PreviewLine {
        text: text.into(),
        flagged_word: Some(word.into()),
    }
}

/// Fixture: session list for the left panel — codes and red-dot flags match
/// the design mockups (`Example Imgs/ModMainPanal.png`).
pub(super) fn sample_sessions() -> Vec<ChatSession> {
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

/// Fixture: flagged-message overview for the landing page. Body IDs use the
/// 12-char alphanumeric shape the server will assign once wired.
pub(super) fn sample_flagged() -> Vec<FlaggedSession> {
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

/// Fixture: transcript for the session view — a flagged exchange for
/// FJ5B3V (mirroring `Example Imgs/ModSessionEntered.png`), a clean generic
/// one for the other demo sessions so every card is enterable.
pub(super) fn sample_transcript(code: &str) -> Vec<ChatMessage> {
    if code == "FJ5B3V" {
        vec![
            ChatMessage {
                body_id: "T09XB4N6QW22".into(),
                username: "PixelPirate".into(),
                player_id: Some(12),
                body: "that portal save tho".into(),
                flagged_word: None,
                ..Default::default()
            },
            ChatMessage {
                body_id: "W34V67898701".into(),
                username: "Warstorm".into(),
                player_id: Some(7),
                body: "frick you all".into(),
                flagged_word: Some("frick".into()),
                ..Default::default()
            },
            ChatMessage {
                body_id: "J55RD2H8PL04".into(),
                username: "PixelPirate".into(),
                player_id: Some(12),
                body: "chill, it is one point".into(),
                flagged_word: None,
                ..Default::default()
            },
            ChatMessage {
                body_id: "Q71ZT8C3VB55".into(),
                username: "Warstorm".into(),
                player_id: Some(7),
                body: "no frick this whole game".into(),
                flagged_word: Some("frick".into()),
                ..Default::default()
            },
        ]
    } else {
        vec![
            ChatMessage {
                body_id: "A12BC3D4E5F6".into(),
                username: "PlayerOne".into(),
                player_id: Some(101),
                body: "good game so far".into(),
                flagged_word: None,
                ..Default::default()
            },
            ChatMessage {
                body_id: "G78HJ9K1L2M3".into(),
                username: "PlayerTwo".into(),
                player_id: Some(102),
                body: "watch the corner barrier".into(),
                flagged_word: None,
                ..Default::default()
            },
        ]
    }
}

/// A snapshot message, shorthand for the sample data below.
pub(super) fn snap(body_id: &str, username: &str, player_id: u64, body: &str, word: Option<&str>) -> ChatMessage {
    ChatMessage {
        body_id: body_id.into(),
        username: username.into(),
        player_id: Some(player_id),
        body: body.into(),
        flagged_word: word.map(Into::into),
        ..Default::default()
    }
}

/// Fixture: audit records for the three Chat Audit Logs category tables.
///
/// **Player** exercises the per-(action, player) model: a multi-body Warn +
/// Delete (mirrors `Example Imgs/ChatAuditLog.png`), a bulk Ban split across two
/// players into same-timestamp rows, and EldenFire recurring across the session.
/// **Word** covers a global Blacklist (no player/body) and an Approve occurrence
/// (with sender + body + snapshot).
///
/// **List** is a *view*, not a fifth store: its rows are the same records as the
/// Player rows they duplicate here, reached through the list index because they
/// carry a `list` tag. The bans and the un-ban therefore appear in both vectors
/// on purpose — that is the behaviour under test, not a fixture mistake.
///
/// Body IDs use the 12-char alphanumeric shape the server will assign once wired.
pub(super) fn sample_audit_log() -> AuditLog {
    AuditLog {
        window_label: "1-100".into(),
        window_notice: None,
        players: vec![
            // Lifting a ban is an action on a player, so it belongs here beside
            // the ban it reverses — the same record the List table shows. No
            // snapshot: an un-ban has no conversation that prompted it.
            PlayerAuditEntry {
                timestamp: "2026-07-24 15:10:33 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Remove Ban".into(),
                reason: "Appeal granted".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                body_ids: vec![],
                snapshot_cut: None,
                flagged_words: vec![],
                snapshot: vec![],
            },
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
                snapshot_cut: None,
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
                snapshot_cut: None,
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
                snapshot_cut: None,
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
                // A ban keeps the whole conversation, so its snapshot runs past
                // the action — the cut is what divides evidence from aftermath.
                snapshot_cut: Some(2),
                flagged_words: vec!["frick".into()],
                snapshot: vec![
                    snap("Q71ZT8C3VB55", "EldenFire", 12, "frick the mods", Some("frick")),
                    snap("B19HN5J8WD30", "MossyOak", 3, "get rekt scrub", Some("scrub")),
                    snap("Z66GT1Y5CV18", "RallyKnight", 34, "well that escalated", None),
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
                snapshot_cut: None,
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
                snapshot_cut: None,
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
            // The two Ban rows above, seen from the List table. Not copies —
            // the same two records, reached through the list index because they
            // carry a `list` tag. Present here so the render tests can assert a
            // ban shows in both tables.
            ListAuditEntry {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Ban".into(),
                reason: "Slur spam".into(),
                target_username: "EldenFire".into(),
                target_player_id: 12,
                list: "Ban List".into(),
            },
            ListAuditEntry {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                moderator_display: "Nova".into(),
                moderator_group: "Moderator".into(),
                action: "Ban".into(),
                reason: "Slur spam".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                list: "Ban List".into(),
            },
            ListAuditEntry {
                timestamp: "2026-07-24 15:10:33 UTC".into(),
                moderator_display: "Warstorm".into(),
                moderator_group: "Admin".into(),
                action: "Remove Ban".into(),
                reason: "Appeal granted".into(),
                target_username: "MossyOak".into(),
                target_player_id: 3,
                // Matches `chat::bans::AUDIT_LIST_NAME`, which both ban paths and
                // the un-ban path tag their records with — and the `List` filter
                // dropdown's option text, so filtering on it will find these.
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

/// Fixture: Moderation Lists data for the three wired sub-tabs (Whitelisted
/// Users has no mockup yet). Reuses the demo players/words from the audit sample
/// so every table reads coherently against the rest of the panel.
pub(super) fn sample_moderation_lists() -> ModerationLists {
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
            // A ban from the session view keeps the whole conversation, with the
            // cut marking where it fell — the two lines after index 2 are what a
            // reviewer must not mistake for the evidence that led to it.
            BannedUser {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                username: "EldenFire".into(),
                player_id: 12,
                reason: "Slur spam".into(),
                snapshot: vec![
                    snap("R21WQ7H4NM08", "RallyKnight", 34, "nice portal defense", None),
                    snap("L88KD3F1QA72", "EldenFire", 12, "frick that was my ball", Some("frick")),
                    snap("T09XB4N6QW22", "EldenFire", 12, "frick this whole match", Some("frick")),
                    snap("M04TC9V2HG61", "RallyKnight", 34, "well that escalated", None),
                ],
                snapshot_cut: Some(3),
            },
            // A ban from this page has no session context, so no transcript to
            // open — the ledger shows an em-dash rather than an empty overlay.
            BannedUser {
                timestamp: "2026-07-24 13:58:20 UTC".into(),
                username: "MossyOak".into(),
                player_id: 3,
                reason: "Slur spam".into(),
                snapshot: Vec::new(),
                snapshot_cut: None,
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
