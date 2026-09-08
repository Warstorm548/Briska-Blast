using System;
using System.Collections.Generic;

namespace BriskaBlast.Net;

/// <summary>Everything this connection surfaces to the rest of the client. Each is
/// raised from the frame dispatch in SignalingClient.FrameHandler.cs, on Godot's
/// main thread, so handlers may touch the scene tree directly.</summary>
public partial class SignalingClient
{
    /// <summary>Identify accepted. Carries (hostPlayerId, peers, seatOrder,
    /// selfIsHost, usernames, iceServers). <c>peers</c> excludes self (WebRTC mesh
    /// targets); <c>seatOrder</c> is the frozen, self-inclusive Extended-mode
    /// seating roster ([host, …joiners] in join order, empty until the match has
    /// started) used to lay out portals. <c>usernames</c> maps player_id → display
    /// name for the ids in this frame (host + self + peers); ids with no server
    /// username are absent, so consumers fall back to <c>Player &lt;id&gt;</c>.
    /// <c>iceServers</c> is only populated on a mid-game identify (the server
    /// resends the match's TURN credential set so a process-death rejoiner can
    /// re-mesh); empty in the lobby, on old servers, and when TURN is off — feed
    /// it to <see cref="WebRtcMeshTransport.SetIceServers"/> before
    /// connecting.</summary>
    public event Action<string, string[], string[], bool, Dictionary<string, string>, IceServerDto[]>? Identified;
    /// <summary>A peer completed identify. Carries (playerId, username); username
    /// is empty when none is on file.</summary>
    public event Action<string, string>? PeerJoined;
    public event Action<string, string>? PeerLeft;
    public event Action<string>? HostChanged;
    /// <summary>Match starting. Carries (gamemode, winCondition, spawnSettings,
    /// lootSettings, playerCount, peers, iceServers). <c>winCondition</c>,
    /// <c>spawnSettings</c> and <c>lootSettings</c> are the host-chosen rules every
    /// client applies. <c>iceServers</c> is the match's server-minted STUN+TURN list
    /// (empty on old servers or when TURN is off — the transport then keeps its
    /// STUN-only fallback); feed it to
    /// <see cref="WebRtcMeshTransport.SetIceServers"/> before connecting.</summary>
    public event Action<string, WinConditionDto, SpawnSettingsDto, LootSettingsDto, int, string[], IceServerDto[]>? StartSignaling;
    public event Action<string>? SessionEnded;
    public event Action<string>? Kicked;
    /// <summary>Authoritative per-session score tally (player_id → points)
    /// broadcast by the server after a score report. Overwrite, don't add.</summary>
    public event Action<Dictionary<string, int>>? ScoreUpdate;
    /// <summary>The win condition was met — the match is over. Carries
    /// (winnerPlayerId, finalScores). A pure UI signal: freeze the sim and show the
    /// end-game leaderboard. The server tears the session down via a following
    /// <see cref="SessionEnded"/>, which the game scene ignores once game-over.</summary>
    public event Action<string, Dictionary<string, int>>? GameOver;
    /// <summary>The ready barrier resolved: every player's mesh is up (or the
    /// server's grace valve fired) and the match is on. The client holds on the
    /// connecting screen after its own mesh completes until this arrives —
    /// broadcast at barrier resolution, or a direct reply to a late
    /// <see cref="SendClientReady"/> — so nobody serves into a mesh a slower
    /// peer hasn't finished opening.</summary>
    public event Action? MatchStarted;
    /// <summary>A process-death rejoiner is re-entering the live match: the
    /// server paused everyone while it re-meshes. Carries (playerId, username,
    /// resumeTimeoutSecs) — username empty when none on file. Resolved by
    /// <see cref="MatchResumed"/> within at most resumeTimeoutSecs.</summary>
    public event Action<string, string, int>? MatchPaused;
    /// <summary>The pause ended (rejoiner meshed, dropped again, or the server's
    /// valve fired). Carries countdownSecs — run a countdown, then unfreeze.</summary>
    public event Action<int>? MatchResumed;
    /// <summary>A lobby chat message arrived. The server broadcasts to everyone
    /// including the sender, so this also fires for this client's own messages —
    /// render player lines all the same way. See <see cref="ChatLine"/> for the
    /// moderator case, which carries no sender id.</summary>
    public event Action<ChatLine>? ChatMessage;
    /// <summary>A moderator sent this player a warning. Carries the reason. Sent
    /// to the targeted player alone, never broadcast, and never queued — if it
    /// arrives at all it is because this client was connected and in the lobby
    /// when the moderator acted.
    ///
    /// Deliberately carries no moderator identity: the reason is the whole
    /// message.</summary>
    public event Action<string>? ChatWarning;
    /// <summary>This player's chat privileges were revoked. Carries the reason.
    /// Sent to the banned player alone.
    ///
    /// Unlike a warning this can arrive more than once: when the ban is applied,
    /// and again each time the server refuses a chat message from them. That
    /// repeat is what makes the notice reach a player who was offline when the
    /// ban landed, without the server queueing anything.
    ///
    /// Carries no moderator identity, same as <see cref="ChatWarning"/>.</summary>
    public event Action<string>? ChatBanned;
    /// <summary>A moderator withdrew a chat line from every player in the
    /// session. Carries the <see cref="ChatLine.BodyId"/> of the line to remove.
    /// Fires for a message this client may never have seen (it may have joined
    /// after), in which case there is simply nothing to remove.</summary>
    public event Action<string>? ChatBodyDeleted;
    /// <summary>Socket closed for good (deliberate close, an auth-level
    /// rejection, or the reconnect window expired). Carries the close code
    /// (4xxx app codes from the server, or transport codes like 1006) and the
    /// reason string.</summary>
    public event Action<int, string>? Closed;
    /// <summary>The WS dropped unexpectedly and an automatic reconnect loop has
    /// begun — the session is not abandoned yet, so UI may show a
    /// "reconnecting…" state instead of leaving.</summary>
    public event Action? Reconnecting;
    /// <summary>The automatic reconnect succeeded and <c>identify</c> was
    /// re-sent; the session resumes on the same connection.</summary>
    public event Action? Reconnected;
    /// <summary>The host dropped mid-game and the server armed a reconnect grace
    /// window. Carries (hostPlayerId, graceSecs).</summary>
    public event Action<string, int>? HostReconnecting;
    /// <summary>A dropped host returned within the grace window; the host role
    /// is unchanged.</summary>
    public event Action<string>? HostReconnected;
    /// <summary>A non-host peer dropped mid-game and the server armed a reconnect
    /// window. Carries (playerId, graceSecs) so peers can show a
    /// "reconnecting…" overlay. Resolved by <see cref="PeerJoined"/> (they
    /// rejoined) or <see cref="PeerLeft"/> (the window elapsed).</summary>
    public event Action<string, int>? PeerReconnecting;

    // WebRTC negotiation relays (server attests `from`). The transport layer
    // subscribes to these and feeds them into the matching peer connection.
    public event Action<string, string>? OfferReceived;          // (from, sdp)
    public event Action<string, string>? AnswerReceived;         // (from, sdp)
    public event Action<string, string, string, int>? IceCandidateReceived; // (from, candidate, sdpMid, sdpMLineIndex)
}
