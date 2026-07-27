//! The landing page and the entered-session view.

use super::fixtures::{sample_flagged, sample_moderation_lists, sample_sessions, sample_transcript};
use crate::admin::templates::{self, ChatMessage};

#[test]
fn session_page_wires_the_moderator_chat_bar() {
    let html = templates::chatmod_session_page(
        "FJ5B3V",
        &sample_transcript("FJ5B3V"),
        &sample_sessions(),
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    // Enter and the Send button run the same path, so desktop and the mobile
    // drawer layout behave identically.
    assert!(html.contains(r#"onkeydown="if(event.key==='Enter'){event.preventDefault();bbCmSay();}""#));
    assert!(html.contains(r#"onclick="bbCmSay()""#));
    assert!(html.contains(r#"id="cm-chatbar-input""#));
    // Bounded the same way a player message is.
    assert!(html.contains(r#"maxlength="500""#));
    // The anonymity toggle is addressable and defaults to unchecked, i.e.
    // anonymous — a moderator must opt in to showing their name.
    assert!(html.contains(r#"<input type="checkbox" id="cm-show-name"> Appear As Your Display Name"#));
    // The poller learns which session it is in from the body attribute.
    assert!(html.contains(r#"<body data-cm-code="FJ5B3V">"#));
    // Refresh targets exist for the panels the poll swaps.
    assert!(html.contains(r#"id="cm-chat""#));
    assert!(html.contains(r#"id="cm-sessions""#));
    // A failed send gives the message back rather than eating it silently.
    assert!(html.contains("function restore()"));
    assert!(html.contains("}).catch(restore);"));
    assert!(html.contains("if(!r.ok){restore();return;}"));
    // The poller holds one request at a time and idles on a hidden tab.
    assert!(html.contains("if(busy)return;"));
    assert!(html.contains("if(!document.hidden)load();"));
    assert!(html.contains("visibilitychange"));
}

#[test]
fn landing_page_polls_without_a_session_context() {
    let html = templates::chatmod_landing_page(
        &sample_sessions(),
        &sample_flagged(),
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    // No session entered, so no code — the poller falls back to the landing
    // endpoint rather than asking for a transcript that doesn't exist.
    assert!(html.contains("<body>"));
    // (the attribute name still appears in the poller's script — what matters
    // is that the body tag carries no value for it)
    assert!(!html.contains("<body data-cm-code"));
    assert!(html.contains(r#"id="cm-flagged""#));
}

#[test]
fn moderator_line_renders_without_a_player_id() {
    let transcript = vec![
        ChatMessage {
            body_id: "a00000000001".into(),
            username: "Warstorm".into(),
            player_id: Some(7),
            body: "nice shot".into(),
            flagged_word: None,
            is_moderator: false,
            posted_as: None,
        },
        ChatMessage {
            body_id: "a00000000002".into(),
            username: "jeanluc".into(),
            player_id: None,
            body: "keep it civil".into(),
            flagged_word: None,
            is_moderator: true,
            posted_as: Some("Mod".into()),
        },
    ];
    let html = templates::chatmod_session_page(
        "FJ5B3V",
        &transcript,
        &sample_sessions(),
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    // The moderator's line is tagged, names the real moderator, and discloses
    // that players saw a generic label — so colleagues in the same session can
    // tell two anonymous moderators apart.
    assert!(html.contains(
        r#"<span class="cm-msg-mod">MOD</span> jeanluc <span class="cm-msg-as">as &ldquo;Mod&rdquo;</span>"#
    ));
    // ...and no select checkbox, because the player tools act on player
    // accounts and a moderator is not one.
    assert_eq!(
        html.matches(r#"aria-label="Select message"#).count(),
        1,
        "only the player line should be selectable"
    );
    // The player's line still shows theirs.
    assert!(html.contains(r#"data-copy="000000007""#));
    assert!(!html.contains("ID 000000000"), "a moderator must never render a zero id");
}

// Session resolution now hits Redis, so the half that can be unit-tested is
// the shape guard — see `chatmod_data::canonical_code` for the rest of this
// coverage. What matters at this layer is that an unresolvable code sends the
// moderator back to the landing page rather than being reflected into a link.
#[test]
fn unresolved_session_falls_back_to_the_landing_page() {
    let html = templates::chatmod_lists_page(
        &sample_moderation_lists(),
        &sample_sessions(),
        None,
        None,
        crate::admin::AdminRole::Moderator,
        "modtester",
    );
    assert!(html.contains(r#"href="/admin/chatmod" class="cm-close""#));
    assert!(!html.contains("?from="), "a None context must not emit a from param");
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
