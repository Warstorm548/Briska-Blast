use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::turn::IceServer;
use shared::types::gamemode::GameMode;
use shared::types::spawn_settings::SpawnSettings;
use shared::types::win_condition::WinCondition;

/// Default points credited by a `ReportScore` that omits the field (older clients
/// send a single point). The server clamps the accepted value to a sane maximum.
fn default_score_points() -> i64 {
    1
}

/// Client → server signaling frames. JSON-tagged on `type`. Receive-only,
/// so Deserialize but not Serialize. The server attests `from` on relays
/// based on the authenticated WS connection — clients only specify `to`
/// for targeted messages; they cannot forge a `from` field.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Must be the first frame after upgrade. Carries auth so the server
    /// can bind the WS connection to a player.
    ///
    /// `rejoin` distinguishes a **process-death rejoin** (a fresh process
    /// re-entering a live match — the match pauses while it re-meshes, see
    /// `MatchPaused`) from a transient WS auto-reconnect (same process, mesh
    /// intact — never pauses). Clients set it only on their rejoin paths;
    /// defaulted so older clients read as non-rejoin.
    Identify {
        player_id: String,
        secret_token: String,
        #[serde(default)]
        rejoin: bool,
    },
    /// SDP offer aimed at a specific peer in the same session.
    Offer { to: String, sdp: String },
    /// SDP answer aimed at a specific peer.
    Answer { to: String, sdp: String },
    /// ICE candidate aimed at a specific peer.
    IceCandidate {
        to: String,
        candidate: String,
        sdp_mid: String,
        sdp_m_line_index: u16,
    },
    /// Sent by a peer when it has exhausted ICE candidates against `peer`
    /// and cannot establish a direct connection (symmetric NAT case). The
    /// server's full handling of this is deferred to a follow-up branch;
    /// for now the server logs it.
    PeerConnectionFailed {
        peer: String,
        reason: String,
    },
    /// Voluntary clean teardown. Server will treat the WS close as a
    /// normal disconnect.
    Leave,
    /// Reports that a ball got past a player's paddle into their goal and
    /// `scoring_player_id` (the last player to have hit the ball) should be
    /// credited `points` (1 for the master ball, 2 for a BallBT split ball).
    /// Scores are server-relayed rather than P2P so the server owns the canonical
    /// tally — a hook for later trajectory validation. The server currently trusts
    /// any session member's report but clamps `points` to a sane range. The field
    /// is optional on the wire so an older client (no `points`) still credits 1.
    ReportScore {
        scoring_player_id: String,
        #[serde(default = "default_score_points")]
        points: i64,
    },
    /// Clock-sync probe. `client_send_ms` is the client's own monotonic send
    /// time, echoed back unchanged in the `TimeSync` reply so the client can
    /// compute round-trip delay and its offset to the server clock without the
    /// server tracking any per-connection state. See `ServerClock` (client).
    TimeSync { client_send_ms: i64 },
    /// A lobby chat message typed by this player. Relayed through signaling
    /// (the lobby has no WebRTC mesh yet) so the server can attest the sender
    /// and resolve their display name, then broadcast `ChatMessage` to the whole
    /// room. The server trims the text, bounds its length, and drops empties.
    SendChat { text: String },
    /// Sent once this client's WebRTC mesh is fully up after `StartSignaling`
    /// (every expected peer's data channel open) — the client's half of the
    /// ready barrier. When every seated player has reported ready (or the
    /// server's grace valve fires first), the server flips the session
    /// `starting → active` and broadcasts `MatchStarted`. A late ready after
    /// the match already started gets a direct `MatchStarted` reply, so
    /// stragglers and rejoiners converge on the same "send ready, wait for
    /// match_started" contract.
    ClientReady,
}

/// Who produced a `ChatMessage`. Serializes to a bare `"player"` / `"moderator"`
/// string so the field reads naturally on the wire and stays cheap to extend.
///
/// Kept deliberately coarse: the client needs only enough to style the line, not
/// the moderator's role or identity. Which moderator actually spoke — including
/// behind an anonymous `Mod` label — is recorded server-side in the transcript,
/// never broadcast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    #[default]
    Player,
    Moderator,
}

/// Server → client signaling frames. JSON-tagged on `type`. Cloneable
/// because broadcasts fan one message out to multiple senders.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent in reply to a successful Identify. Tells the client which player
    /// the server has bound to this connection, who the authoritative host
    /// is, plus the current lobby roster at identify time so the client
    /// doesn't need a separate poll.
    ///
    /// `usernames` maps the player_ids appearing in this frame (the union of
    /// `your_player_id`, `host_player_id`, and `peers`) to their server-stored
    /// display names, so the client can label the lobby/scoreboard by username
    /// instead of the internal id. Ids with no stored username are omitted —
    /// the client falls back to `Player <id>`.
    ///
    /// `seat_order` is the frozen, self-inclusive seating roster (`[host,
    /// ...joiners]` in join order) snapshotted at `/start`; empty while the
    /// session is still Waiting. Unlike `peers` (which excludes self, for
    /// meshing), this is identical on every client and includes the recipient,
    /// so a process-death rejoiner can reproduce the exact Extended-mode portal
    /// layout the rest of the match froze at Start. See `GameScene.BuildEdges`.
    ///
    /// `ice_servers` carries freshly minted STUN+TURN entries **only when the
    /// session is already past Start** (`seat_order` non-empty) — i.e. for a
    /// process-death rejoiner, a fresh process that missed the credentials in
    /// `StartSignaling`. Empty while Waiting (lobby members get theirs at
    /// Start) and when TURN is unconfigured / the mint failed, in which case
    /// the client keeps its built-in STUN-only fallback.
    Identified {
        your_player_id: String,
        host_player_id: String,
        peers: Vec<String>,
        seat_order: Vec<String>,
        is_host: bool,
        usernames: HashMap<String, String>,
        ice_servers: Vec<IceServer>,
    },
    /// Broadcast when a new player completes Identify in this session.
    /// `username` is the joining player's server-stored display name (empty if
    /// none on file — the client falls back to `Player <id>`).
    PeerJoined { player_id: String, username: String },
    /// Broadcast when a player disconnects or is removed. `reason` is a
    /// closed-set discriminator so clients can decide UI/recovery behavior.
    PeerLeft {
        player_id: String,
        reason: &'static str,
    },
    /// Broadcast when a non-host player's WebSocket drops mid-game (past
    /// Waiting). Peers show a "a player is reconnecting…" overlay for
    /// `grace_secs`; their slot is held for a longer rejoin window. Resolved by
    /// either `PeerJoined` (they re-Identified and rejoin — the mesh re-meshes)
    /// or `PeerLeft { reason: "reconnect_timeout" }` (the window elapsed). The
    /// host's equivalent is `HostReconnecting` (which also drives promotion).
    PeerReconnecting { player_id: String, grace_secs: u64 },
    /// Broadcast when the host role moves to a different player. Fired by a
    /// voluntary `/session/:code/host` transfer (lobby) and by automatic
    /// join-order promotion when a disconnected host fails to return within the
    /// grace window (`promote_or_end_active`, see `ws.rs`).
    HostChanged { player_id: String },
    /// Broadcast when the host's WebSocket drops mid-game (past Waiting) and the
    /// server has armed a reconnect grace window. Peers show a "host
    /// reconnecting…" state and keep playing; the ball still flows over the
    /// (independent) WebRTC mesh. Resolved by either `HostReconnected` (the host
    /// re-Identified in time) or `HostChanged` / `SessionEnded` (grace expired).
    HostReconnecting { player_id: String, grace_secs: u64 },
    /// Broadcast when a host that triggered `HostReconnecting` re-Identifies
    /// within the grace window. Peers clear the "host reconnecting…" state; the
    /// host role is unchanged.
    HostReconnected { player_id: String },
    /// Broadcast when the host calls /start. Clients begin WebRTC negotiation
    /// on receipt. `peers` is the authoritative roster at start time.
    /// `win_condition` is the host-chosen match-end rule (mirrors `gamemode`), so
    /// every joiner applies the same rule when the server signals game over.
    /// `ice_servers` is the match's freshly minted STUN+TURN list (one
    /// credential set shared by the whole match, TTL outlives it — see
    /// `turn.rs`). Empty when TURN is unconfigured or the mint failed; the
    /// client then keeps its built-in STUN-only fallback.
    StartSignaling {
        gamemode: GameMode,
        win_condition: WinCondition,
        /// Host-chosen random-spawn rules (BallSpliter cadence + chain-split), so
        /// every client drives an identical local spawner.
        spawn_settings: SpawnSettings,
        player_count: u8,
        peers: Vec<String>,
        ice_servers: Vec<IceServer>,
    },
    /// Relayed SDP offer. `from` is server-attested — set from the
    /// authenticated WS connection, not echoed from the client's frame.
    Offer { from: String, sdp: String },
    /// Relayed SDP answer.
    Answer { from: String, sdp: String },
    /// Relayed ICE candidate.
    IceCandidate {
        from: String,
        candidate: String,
        sdp_mid: String,
        sdp_m_line_index: u16,
    },
    /// Sent to every member just before the WS closes when the session ends.
    SessionEnded { reason: &'static str },
    /// Broadcast once the win condition is met: the match is over. A **pure UI
    /// signal** — clients freeze the simulation and show the end-game leaderboard
    /// with Return-to-Menu / Host-Game actions. It carries no cleanup semantics;
    /// the server hands actual session teardown to the existing `SessionEnded`
    /// path immediately after (see `frame.rs`). `winner_player_id` is the player
    /// who reached the target; `scores` is the final authoritative tally so the
    /// leaderboard is exact even if a `ScoreUpdate` was missed.
    GameOver {
        winner_player_id: String,
        scores: HashMap<String, i64>,
    },
    /// Sent to a single player when the server is removing them from the
    /// session (symmetric-NAT failure, duplicate identify, etc.).
    Kicked { reason: &'static str },
    /// Broadcast after a `ReportScore` is accepted. Carries the full
    /// authoritative per-session tally (player_id → points) so every client
    /// overwrites its scoreboard to match the server rather than tracking
    /// deltas — a dropped/duplicated frame can't desync the score.
    ScoreUpdate { scores: HashMap<String, i64> },
    /// Reply to a `TimeSync` probe. Echoes the client's `client_send_ms` and
    /// carries the server's wall-clock time at reply (`server_ms`). The client
    /// estimates `offset = server_ms − (client_send_ms + recv_ms) / 2` so all
    /// peers can stamp ball handoffs in a shared (server) time frame, making the
    /// transit fast-forward immune to per-machine wall-clock skew.
    TimeSync { client_send_ms: i64, server_ms: i64 },
    /// Broadcast after an accepted `SendChat`. `from` is the server-attested
    /// sender id; `username` is their server-stored display name (empty when none
    /// is on file — the client falls back to `Player <id>`). Sent to everyone
    /// including the sender so all clients render an identical, server-ordered
    /// transcript (same rationale as `ScoreUpdate`).
    ///
    /// `text` is what players should see, which is **not** always what was typed:
    /// blacklisted words are masked here, before broadcast, so the raw word never
    /// leaves the server. The uncensored original is kept only in the moderation
    /// transcript. Censoring therefore needs no client change at all.
    ///
    /// `kind` distinguishes a player line from a moderator speaking into the
    /// session (see `admin::chatmod`). Moderator lines carry an empty `from` — a
    /// moderator has no player id — and `username` is either their Pocket ID
    /// display name or the generic `Mod`, depending on the anonymity toggle they
    /// chose. Clients predating this field ignore it and render the line as an
    /// ordinary chat message, which is a correct (if unstyled) degradation, so
    /// this addition does not require a `min_game_version` bump.
    ///
    /// `body_id` labels this line so a later [`ServerMsg::ChatBodyDeleted`] can
    /// name it. It is not a lookup handle — nothing lets a client ask the server
    /// for a message — it is a tag the client keeps beside a line it already
    /// received, so a delete order can identify *which* line on screen to erase.
    /// Matching on text instead would be ambiguous the moment two messages read
    /// the same, or the broadcast body was censored and no longer matches what
    /// was typed.
    ///
    /// Unlike `kind`, this is not a graceful degradation: a client that ignores
    /// it cannot act on a delete at all and keeps showing a removed message.
    /// That is what requires the `min_game_version` bump.
    ChatMessage {
        from: String,
        username: String,
        text: String,
        kind: ChatKind,
        body_id: String,
    },
    /// A moderator warning, sent to **one player only** — never broadcast. The
    /// reason is what the moderator typed into the Quick Access Tools panel.
    ///
    /// Deliberately carries no moderator identity, anonymous or otherwise: a
    /// warning reads as coming from "a moderator". The anonymity toggle is scoped
    /// to moderator *chat*, and the audit record holds the real identity either
    /// way, so putting a name on the wire would only widen what a client learns.
    ///
    /// Delivery is best-effort and never queued. `SignalHub::send_to` reports
    /// whether the frame reached a live socket, and that outcome is recorded on
    /// the audit record. Note that a live socket is necessary but not sufficient:
    /// chat is rendered only in the lobby, so a player already in a match has a
    /// healthy socket and no surface to show this on. Callers must treat
    /// in-match players as undeliverable rather than trusting `send_to` alone.
    ChatWarning { reason: String },
    /// A player's chat privileges have been revoked, carrying the reason the
    /// moderator gave. Sent to **one player only**, and rendered red rather than
    /// the warning's amber — the colour is the difference between a one-off
    /// notice and a permanent one.
    ///
    /// Carries no moderator identity, for the same reason [`ServerMsg::ChatWarning`]
    /// does not.
    ///
    /// # Two send sites
    ///
    /// 1. When the ban is applied, if the target is live in a lobby. Best-effort
    ///    and never queued, exactly like a warning: an offline or in-match player
    ///    does not receive it and the attempt is recorded as undelivered.
    /// 2. Whenever a banned player attempts to send chat — the frame handler
    ///    refuses the message and answers with this instead of broadcasting.
    ///
    /// The second site is what makes delivery durable without a queue. A player
    /// who missed the first notice learns of the ban the moment they try to
    /// speak, which is the only moment it actually affects them.
    ///
    /// A client that ignores this frame shows nothing at all while the panel
    /// reports the ban applied — a silent false positive, which is what requires
    /// the `min_game_version` bump.
    ChatBanned { reason: String },
    /// Remove a previously-broadcast chat line from every client in the session.
    /// `body_id` matches the field of the same name on [`ServerMsg::ChatMessage`].
    ///
    /// Clients wipe the line's text but **keep the id as a placeholder** in their
    /// ordered chat list, rendering skips it so the visible log closes up with no
    /// gap. That preserves the line's original position for a possible future
    /// restore, which could then refill the hole in place without the server
    /// having to describe where it went. The placeholder lives only as long as
    /// the client's lobby chat does — leaving the session discards it.
    ///
    /// Server-side nothing is removed. The transcript is append-only because
    /// audit records pin a cut index into it, so a deletion is recorded as a mark
    /// in a side key and the list itself is never touched.
    ///
    /// **Trap for any future restore:** the transcript stores the *uncensored*
    /// original, while players only ever saw the masked form. Re-sending the
    /// stored text would leak the exact word the mask existed to hide. A restore
    /// must re-mask against the blacklist before it goes on the wire.
    ChatBodyDeleted { body_id: String },
    /// The ready barrier resolved: every seated player reported `ClientReady`
    /// (or the server's grace valve fired) and the session is now `Active`.
    /// Clients hold on the connecting screen until this arrives, so nobody can
    /// serve into a mesh a slower peer hasn't finished opening. Broadcast to
    /// the room at barrier resolution and additionally sent directly to any
    /// straggler whose `ClientReady` arrives after the fact. An empty struct
    /// (not a unit) so fields can be added compatibly later.
    MatchStarted {},
    /// Broadcast when a process-death rejoiner (`Identify { rejoin: true }`)
    /// re-enters a live match: everyone freezes their sim behind a "Waiting
    /// for {username}…" overlay while the rejoiner re-meshes, so balls sent at
    /// its edge don't bounce off a temporary wall. Resolved by `MatchResumed`
    /// — fired by the rejoiner's `ClientReady`, by its disconnecting again, or
    /// by the server's `resume_timeout_secs` valve, whichever is first.
    /// `username` is the rejoiner's display name (empty when none on file).
    /// Never fired for a transient WS reconnect or the initial mid-game drop.
    MatchPaused {
        player_id: String,
        username: String,
        resume_timeout_secs: u64,
    },
    /// Broadcast when the last outstanding pause hold clears (multi-rejoiner
    /// safe): clients run a `countdown_secs` 3-2-1 and unfreeze together. The
    /// rejoiner itself is still on the connecting screen when this fires — its
    /// own go-signal stays `MatchStarted`.
    MatchResumed { countdown_secs: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player line must keep the exact three fields older clients read, with
    /// `kind` and `body_id` added alongside them. Adding a field is safe
    /// precisely because the C# dispatcher pulls named properties rather than
    /// deserializing a struct.
    #[test]
    fn chat_message_serializes_with_kind() {
        let msg = ServerMsg::ChatMessage {
            from: "000000007".into(),
            username: "Warstorm".into(),
            text: "nice shot".into(),
            kind: ChatKind::Player,
            body_id: "a00000000005".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json,
            r#"{"type":"chat_message","from":"000000007","username":"Warstorm","text":"nice shot","kind":"player","body_id":"a00000000005"}"#
        );
    }

    /// A moderator line carries an empty `from` — a moderator has no player id —
    /// and the display name players actually see, which for an anonymous post is
    /// the generic `Mod`. The real identity behind it is never on the wire.
    #[test]
    fn moderator_chat_message_serializes_without_a_player_id() {
        let msg = ServerMsg::ChatMessage {
            from: String::new(),
            username: "Mod".into(),
            text: "keep it civil".into(),
            kind: ChatKind::Moderator,
            body_id: "a00000000006".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json,
            r#"{"type":"chat_message","from":"","username":"Mod","text":"keep it civil","kind":"moderator","body_id":"a00000000006"}"#
        );
    }

    /// A line the transcript never stored still broadcasts — it just carries no
    /// id, and so cannot be targeted by a later moderation action. Capture
    /// failing must never silence chat.
    #[test]
    fn chat_message_tolerates_an_unrecorded_body() {
        let msg = ServerMsg::ChatMessage {
            from: "000000007".into(),
            username: "Warstorm".into(),
            text: "nice shot".into(),
            kind: ChatKind::Player,
            body_id: String::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""body_id":"""#));
    }

    /// A warning names no moderator. The audit record holds the real identity;
    /// the wire carries only what the player is being told.
    #[test]
    fn chat_warning_serializes_with_only_a_reason() {
        let msg = ServerMsg::ChatWarning {
            reason: "Offensive language".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json,
            r#"{"type":"chat_warning","reason":"Offensive language"}"#
        );
        assert!(
            !json.contains("moderator"),
            "a warning must not identify who sent it"
        );
    }

    /// A ban notice is shaped exactly like a warning on the wire — the client
    /// tells them apart by frame type, not by any field, which is what lets the
    /// two share one banner and differ only in colour.
    #[test]
    fn chat_banned_serializes_with_only_a_reason() {
        let msg = ServerMsg::ChatBanned {
            reason: "Repeated slurs".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"chat_banned","reason":"Repeated slurs"}"#);
        assert!(
            !json.contains("moderator"),
            "a ban notice must not identify who applied it"
        );
    }

    /// The delete order carries the same id the original `ChatMessage` did, and
    /// nothing else — the client already holds everything else about that line.
    #[test]
    fn chat_body_deleted_serializes_with_the_body_id() {
        let msg = ServerMsg::ChatBodyDeleted {
            body_id: "a00000000005".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json,
            r#"{"type":"chat_body_deleted","body_id":"a00000000005"}"#
        );
    }

    /// StartSignaling must carry the minted ICE list on the wire: STUN entries
    /// without credential keys (the client treats key presence as
    /// "credentialed"), TURN entries with them.
    #[test]
    fn start_signaling_serializes_ice_servers() {
        let msg = ServerMsg::StartSignaling {
            gamemode: GameMode::Extended,
            win_condition: WinCondition::default(),
            spawn_settings: SpawnSettings::default(),
            player_count: 2,
            peers: vec!["000000001".into(), "000000004".into()],
            ice_servers: vec![
                IceServer {
                    urls: vec!["stun:stun.cloudflare.com:3478".into()],
                    username: None,
                    credential: None,
                },
                IceServer {
                    urls: vec!["turn:turn.cloudflare.com:3478?transport=udp".into()],
                    username: Some("user".into()),
                    credential: Some("pass".into()),
                },
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"start_signaling""#));
        assert!(json.contains(
            r#""ice_servers":[{"urls":["stun:stun.cloudflare.com:3478"]},"#
        ));
        assert!(json.contains(r#""username":"user","credential":"pass""#));
    }

    /// The client's ready frame is bodyless — just the tag.
    #[test]
    fn client_ready_deserializes_from_bare_tag() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"client_ready"}"#).unwrap();
        assert!(matches!(msg, ClientMsg::ClientReady));
    }

    /// `rejoin` on Identify is optional on the wire: absent (an old client, or
    /// a normal lobby/reconnect identify) reads as false; a rejoin path sends
    /// it explicitly.
    #[test]
    fn identify_rejoin_flag_defaults_to_false() {
        let msg: ClientMsg = serde_json::from_str(
            r#"{"type":"identify","player_id":"000000001","secret_token":"tok"}"#,
        )
        .unwrap();
        assert!(matches!(msg, ClientMsg::Identify { rejoin: false, .. }));

        let msg: ClientMsg = serde_json::from_str(
            r#"{"type":"identify","player_id":"000000001","secret_token":"tok","rejoin":true}"#,
        )
        .unwrap();
        assert!(matches!(msg, ClientMsg::Identify { rejoin: true, .. }));
    }

    /// Pause/resume wire shapes: field names the client parser matches on.
    #[test]
    fn match_paused_and_resumed_serialize_expected_fields() {
        let json = serde_json::to_string(&ServerMsg::MatchPaused {
            player_id: "000000002".into(),
            username: "Zoe".into(),
            resume_timeout_secs: 25,
        })
        .unwrap();
        assert!(json.contains(r#""type":"match_paused""#));
        assert!(json.contains(r#""player_id":"000000002""#));
        assert!(json.contains(r#""username":"Zoe""#));
        assert!(json.contains(r#""resume_timeout_secs":25"#));

        let json = serde_json::to_string(&ServerMsg::MatchResumed { countdown_secs: 3 }).unwrap();
        assert_eq!(json, r#"{"type":"match_resumed","countdown_secs":3}"#);
    }

    /// MatchStarted is an empty struct variant so later fields stay additive;
    /// on the wire today it's just the tag.
    #[test]
    fn match_started_serializes_to_bare_tag() {
        let json = serde_json::to_string(&ServerMsg::MatchStarted {}).unwrap();
        assert_eq!(json, r#"{"type":"match_started"}"#);
    }

    /// A Waiting-phase Identified (no mint) still carries the field as an
    /// explicit empty array — the client parses it unconditionally.
    #[test]
    fn identified_serializes_empty_ice_servers() {
        let msg = ServerMsg::Identified {
            your_player_id: "000000001".into(),
            host_player_id: "000000004".into(),
            peers: vec!["000000004".into()],
            seat_order: vec![],
            is_host: false,
            usernames: HashMap::new(),
            ice_servers: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""ice_servers":[]"#));
    }
}
