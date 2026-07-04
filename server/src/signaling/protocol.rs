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
    Identify {
        player_id: String,
        secret_token: String,
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
    ChatMessage {
        from: String,
        username: String,
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

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
