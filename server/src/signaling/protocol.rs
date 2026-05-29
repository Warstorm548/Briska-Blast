use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use shared::types::gamemode::GameMode;

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
    /// credited a point. Scores are server-relayed rather than P2P so the
    /// server owns the canonical tally — a hook for later trajectory
    /// validation. The server currently trusts any session member's report.
    ReportScore { scoring_player_id: String },
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
    Identified {
        your_player_id: String,
        host_player_id: String,
        peers: Vec<String>,
        is_host: bool,
    },
    /// Broadcast when a new player completes Identify in this session.
    PeerJoined { player_id: String },
    /// Broadcast when a player disconnects or is removed. `reason` is a
    /// closed-set discriminator so clients can decide UI/recovery behavior.
    PeerLeft {
        player_id: String,
        reason: &'static str,
    },
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
    StartSignaling {
        gamemode: GameMode,
        player_count: u8,
        peers: Vec<String>,
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
    /// Sent to a single player when the server is removing them from the
    /// session (symmetric-NAT failure, duplicate identify, etc.).
    Kicked { reason: &'static str },
    /// Broadcast after a `ReportScore` is accepted. Carries the full
    /// authoritative per-session tally (player_id → points) so every client
    /// overwrites its scoreboard to match the server rather than tracking
    /// deltas — a dropped/duplicated frame can't desync the score.
    ScoreUpdate { scores: HashMap<String, i64> },
}
