using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>
/// One chat line as broadcast by the server.
///
/// A record rather than more event arguments: chat has grown a sender kind and
/// will likely grow more, and four positional strings at a call site stop being
/// readable.
/// </summary>
/// <param name="From">Server-attested sender id. <em>Empty for a moderator
/// line</em> — a moderator speaks through the admin panel and has no player
/// account, so this must not be fed to a roster lookup.</param>
/// <param name="Username">The display name to render. For a moderator this is
/// either their real name or the generic <c>Mod</c>, depending on the anonymity
/// toggle they chose; the client is not told which, by design.</param>
/// <param name="Text">The message body. Blacklisted words arrive already masked
/// — the server censors before broadcast, so the raw word never reaches a
/// client and there is nothing to filter here.</param>
/// <param name="IsModerator">True when a moderator spoke into the session
/// rather than a player. Drives the distinct styling.</param>
/// <param name="BodyId">The server's moderation id for this line, and the only
/// identifier a client ever gets for a chat message. It exists so a later
/// <c>chat_body_deleted</c> can name <em>which</em> displayed line to remove;
/// nothing can be looked up with it. Empty when the server did not record the
/// line (a moderation outage never silences chat), in which case the line simply
/// cannot be targeted.</param>
public readonly record struct ChatLine(
    string From,
    string Username,
    string Text,
    bool IsModerator,
    string BodyId);

/// <summary>
/// One player's signaling connection for one session. A <see cref="Node"/>
/// so it can poll the <see cref="WebSocketPeer"/> every frame in
/// <see cref="_Process"/> — which means all events below fire on Godot's
/// main thread and handlers may touch the scene tree directly.
///
/// Sends <c>identify</c> on open and <c>leave</c> on request, and surfaces the
/// lifecycle frames: Identified / PeerJoined / PeerLeft / HostChanged /
/// HostReconnecting / HostReconnected / PeerReconnecting / StartSignaling /
/// SessionEnded / Kicked / Closed, plus ScoreUpdate and the WebRTC relays
/// (offer/answer/ice_candidate) the transport consumes.
///
/// On an <em>unexpected</em> drop it does not give up: it re-dials the same
/// session WS (re-sending <c>identify</c>) for a short window — surfacing
/// Reconnecting / Reconnected — so a transient blip doesn't end the match and
/// the server's host-reconnect grace is actually reachable. Only a deliberate
/// close or an auth-level rejection is terminal (Closed).
/// </summary>
public partial class SignalingClient : Node
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
    /// playerCount, peers, iceServers). <c>winCondition</c> + <c>spawnSettings</c>
    /// are the host-chosen rules every client applies. <c>iceServers</c> is the
    /// match's server-minted STUN+TURN list (empty on old servers or when TURN is
    /// off — the transport then keeps its STUN-only fallback); feed it to
    /// <see cref="WebRtcMeshTransport.SetIceServers"/> before connecting.</summary>
    public event Action<string, WinConditionDto, SpawnSettingsDto, int, string[], IceServerDto[]>? StartSignaling;
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

    private WebSocketPeer _ws = new();
    private string _code = "";
    private string _playerId = "";
    private string _secretToken = "";
    private bool _active;
    private bool _identifySent;
    private bool _closedEmitted;

    // Reconnect state. On an unexpected drop the client re-dials the same
    // session WS (re-sending identify) for up to ReconnectWindowMsec before
    // giving up and surfacing Closed. A deliberate close (_closing) never
    // reconnects. This window is what makes the server's host grace reachable.
    private bool _closing;
    private bool _reconnecting;
    private ulong _reconnectStartMsec;
    private ulong _nextAttemptMsec;
    private const ulong ReconnectWindowMsec = 30_000; // ~ the server's host grace
    private const ulong RetryIntervalMsec = 2_000;

    // Server clock sync. Periodically probes the server over this same WS so ball
    // handoffs can be stamped in a shared time frame (see ServerClock). The first
    // probe rides the open socket right after identify; thereafter every
    // SyncIntervalMsec, and immediately again after a reconnect (the local clock
    // may have stepped while we were away).
    private const ulong SyncIntervalMsec = 12_000;
    private readonly ServerClock _clock = new();
    private ulong _nextSyncMsec;

    private sealed record IdentifyFrame(string Type, string PlayerId, string SecretToken, bool Rejoin);
    private sealed record LeaveFrame(string Type);
    private sealed record OfferFrame(string Type, string To, string Sdp);
    private sealed record AnswerFrame(string Type, string To, string Sdp);
    private sealed record IceFrame(string Type, string To, string Candidate, string SdpMid, int SdpMLineIndex);
    private sealed record PeerConnectionFailedFrame(string Type, string Peer, string Reason);
    private sealed record ReportScoreFrame(string Type, string ScoringPlayerId, int Points);
    private sealed record TimeSyncFrame(string Type, long ClientSendMs);
    private sealed record SendChatFrame(string Type, string Text);
    private sealed record ClientReadyFrame(string Type);

    /// <summary>Open the WS for <paramref name="code"/> and begin identifying.</summary>
    public void Connect(string code, string playerId, string secretToken)
    {
        _code = code;
        _playerId = playerId;
        _secretToken = secretToken;

        // Reset lifecycle flags so a reused instance re-sends identify and can
        // emit Closed again, rather than carrying stale state from a prior run.
        _identifySent = false;
        _closedEmitted = false;
        _active = false;
        _closing = false;
        _reconnecting = false;

        var url = $"{ServerEndpoint.WsBase}/ws/session/{code}";
        var err = _ws.ConnectToUrl(url);
        if (err != Error.Ok)
        {
            Log.Warn("net.signaling", $"ConnectToUrl({url}) failed: {err}");
            EmitClosedOnce(0, $"connect_failed:{err}");
            return;
        }
        _active = true;
    }

    /// <summary>Send a voluntary <c>leave</c> frame (host hands off / joiner
    /// leaves the lobby). The server frees the slot when in Waiting.</summary>
    public void SendLeave()
    {
        // Deliberate departure: never auto-reconnect after this.
        _closing = true;
        if (_ws.GetReadyState() != WebSocketPeer.State.Open)
            return;
        _ws.SendText(JsonSerializer.Serialize(new LeaveFrame("leave"), Json.Options));
        // Flush now: the lobby tears the socket down immediately after this, so
        // without an explicit Poll the queued frame can be dropped before it
        // reaches the wire — and the server would then see a plain disconnect
        // (slot kept for reconnect) instead of an explicit leave (slot freed).
        _ws.Poll();
    }

    /// <summary>Initiate a clean close. Safe to call repeatedly.</summary>
    public void CloseConnection()
    {
        // Deliberate close: suppress the reconnect loop.
        _closing = true;
        if (_active)
            _ws.Close();
    }

    // WebRTC negotiation senders. `to` is the target peer's player_id; the
    // server attests `from` from this authenticated connection.
    public void SendOffer(string to, string sdp) => SendFrame(new OfferFrame("offer", to, sdp));

    public void SendAnswer(string to, string sdp) => SendFrame(new AnswerFrame("answer", to, sdp));

    public void SendIceCandidate(string to, string candidate, string sdpMid, int sdpMLineIndex) =>
        SendFrame(new IceFrame("ice_candidate", to, candidate, sdpMid, sdpMLineIndex));

    /// <summary>Report that a direct connection to <paramref name="peer"/>
    /// could not be established (e.g. ICE exhausted). The server logs it.</summary>
    public void SendPeerConnectionFailed(string peer, string reason) =>
        SendFrame(new PeerConnectionFailedFrame("peer_connection_failed", peer, reason));

    /// <summary>Report that <paramref name="scoringPlayerId"/> (the last player
    /// to hit the ball) scored. The server tallies and broadcasts ScoreUpdate.</summary>
    public void SendReportScore(string scoringPlayerId, int points) =>
        SendFrame(new ReportScoreFrame("report_score", scoringPlayerId, points));

    /// <summary>Send a lobby chat message. The server attests the sender,
    /// resolves the display name, and broadcasts <see cref="ChatMessage"/> to the
    /// whole room (including this client). No-op if the socket isn't open.</summary>
    public void SendChatMessage(string text) =>
        SendFrame(new SendChatFrame("send_chat", text));

    /// <summary>Report that this client's WebRTC mesh is fully up — its half of
    /// the ready barrier. The server answers with <see cref="MatchStarted"/>
    /// once everyone is ready (or immediately if the match already started).
    /// Safe to re-send (e.g. after a WS reconnect); the server treats a
    /// duplicate as a straggler and replies directly.</summary>
    public void SendClientReady() =>
        SendFrame(new ClientReadyFrame("client_ready"));

    /// <summary>Declare this connection's identifies as a process-death rejoin
    /// into a live match — the server then pauses the match while this client
    /// re-meshes. Set by <see cref="Core.MatchFlow"/> only on its rejoin paths
    /// (never for a lobby connect) and cleared once the client is in-match, so
    /// a later transient WS auto-reconnect re-identifies as a normal member and
    /// can't wrongly pause everyone.</summary>
    public bool IdentifyAsRejoin { get; set; }

    /// <summary>Current time in the server-synced frame (ms). Both ends of a ball
    /// handoff stamp/compare with this so cross-machine wall-clock skew cancels
    /// and the transit fast-forward reflects only real network delay. Only
    /// meaningful once <see cref="ClockSynced"/> is true.</summary>
    public long ServerNowMs() => _clock.NowMs((long)Time.GetTicksMsec());

    /// <summary>Whether the server clock offset has at least one sample. Callers
    /// (the handoff fast-forward) should skip time-based correction until this is
    /// true rather than trust an unsynced, machine-local reading.</summary>
    public bool ClockSynced => _clock.Synced;

    private void SendFrame<T>(T frame)
    {
        if (_ws.GetReadyState() == WebSocketPeer.State.Open)
            _ws.SendText(JsonSerializer.Serialize(frame, Json.Options));
    }

    public override void _Process(double delta)
    {
        if (!_active)
            return;

        _ws.Poll();
        var state = _ws.GetReadyState();

        // Deliberate close in progress: don't reconnect — just surface the
        // terminal close once the socket finishes closing.
        if (_closing)
        {
            if (state == WebSocketPeer.State.Closed)
                EmitClosedOnce(_ws.GetCloseCode(), _ws.GetCloseReason());
            return;
        }

        if (_reconnecting)
        {
            ProcessReconnect(state);
            return;
        }

        switch (state)
        {
            case WebSocketPeer.State.Open:
                SendIdentifyOnce();
                MaybeSendTimeSync();
                DrainPackets();
                break;

            case WebSocketPeer.State.Closed:
                OnSocketClosed();
                break;
        }
    }

    private void SendIdentifyOnce()
    {
        if (_identifySent)
            return;
        _ws.SendText(JsonSerializer.Serialize(
            new IdentifyFrame("identify", _playerId, _secretToken, IdentifyAsRejoin), Json.Options));
        _identifySent = true;
    }

    /// <summary>Send a clock-sync probe when one is due. The server only accepts
    /// frames after identify, so gate on that. T1 is captured right before the
    /// send so the round-trip estimate stays tight.</summary>
    private void MaybeSendTimeSync()
    {
        if (!_identifySent)
            return;
        ulong now = Time.GetTicksMsec();
        if (now < _nextSyncMsec)
            return;
        _nextSyncMsec = now + SyncIntervalMsec;
        SendFrame(new TimeSyncFrame("time_sync", (long)now));
    }

    private void DrainPackets()
    {
        while (_ws.GetAvailablePacketCount() > 0)
            HandleFrame(Encoding.UTF8.GetString(_ws.GetPacket()));
    }

    /// <summary>Unexpected close while in a session. App-level rejections
    /// (auth / not in session) are terminal — retrying can't help; anything
    /// else (transport blip, server restart) starts the reconnect loop.</summary>
    private void OnSocketClosed()
    {
        int code = _ws.GetCloseCode();
        if (IsTerminalClose(code))
        {
            EmitClosedOnce(code, _ws.GetCloseReason());
            return;
        }
        _reconnecting = true;
        _reconnectStartMsec = Time.GetTicksMsec();
        _nextAttemptMsec = 0; // first retry on the next tick
        Log.Info("net.signaling", $"connection lost (code {code}) — reconnecting…");
        Reconnecting?.Invoke();
    }

    private void ProcessReconnect(WebSocketPeer.State state)
    {
        switch (state)
        {
            case WebSocketPeer.State.Open:
                // Back up: re-identify on the fresh socket and resume.
                SendIdentifyOnce();
                _nextSyncMsec = 0; // re-sync now: the clock may have stepped while away
                _reconnecting = false;
                Log.Info("net.signaling", "reconnected.");
                Reconnected?.Invoke();
                DrainPackets();
                break;

            case WebSocketPeer.State.Connecting:
                break; // still dialing

            case WebSocketPeer.State.Closed:
                // A reconnect attempt rejected at the app level (auth / not in
                // this session — e.g. an ex-host promoted away) can't succeed by
                // retrying, so bail immediately rather than spin out the window.
                int code = _ws.GetCloseCode();
                if (IsTerminalClose(code))
                {
                    _reconnecting = false;
                    EmitClosedOnce(code, _ws.GetCloseReason());
                    return;
                }
                ulong now = Time.GetTicksMsec();
                if (now - _reconnectStartMsec >= ReconnectWindowMsec)
                {
                    _reconnecting = false;
                    EmitClosedOnce(_ws.GetCloseCode(), "reconnect_failed");
                    return;
                }
                if (now >= _nextAttemptMsec)
                {
                    AttemptReconnect();
                    _nextAttemptMsec = now + RetryIntervalMsec;
                }
                break;
        }
    }

    /// <summary>Dial a fresh peer (no stale close-state lingering); identify
    /// re-sends once it opens. A failed dial just waits for the next interval.</summary>
    private void AttemptReconnect()
    {
        _ws = new WebSocketPeer();
        _identifySent = false;
        var url = $"{ServerEndpoint.WsBase}/ws/session/{_code}";
        var err = _ws.ConnectToUrl(url);
        if (err != Error.Ok)
            Log.Warn("net.signaling", $"reconnect dial failed: {err}");
    }

    // App close codes that mean "you can't be in this session" — reconnecting
    // is futile. Transport codes (e.g. 1006) and -1 (abnormal) are transient.
    private static bool IsTerminalClose(int code) =>
        code == 4401 || code == 4403 || code == 4404;

    private void EmitClosedOnce(int code, string reason)
    {
        if (_closedEmitted)
            return;
        _closedEmitted = true;
        _active = false;
        Closed?.Invoke(code, reason);
    }

    /// <summary>Parse one server frame and dispatch it to the matching event
    /// (<c>identified</c>, <c>peer_joined</c>, host/peer reconnect, score updates,
    /// SDP/ICE relays, chat, …). Malformed or untyped frames are ignored.</summary>
    private void HandleFrame(string text)
    {
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (!root.TryGetProperty("type", out var typeEl))
                return;

            switch (typeEl.GetString())
            {
                case "identified":
                    Identified?.Invoke(
                        Str(root, "host_player_id"),
                        ReadStrings(root, "peers"),
                        ReadStrings(root, "seat_order"),
                        root.GetProperty("is_host").GetBoolean(),
                        ReadStringMap(root, "usernames"),
                        ReadIceServers(root));
                    break;
                case "peer_joined":
                    PeerJoined?.Invoke(Str(root, "player_id"), Str(root, "username"));
                    break;
                case "peer_left":
                    PeerLeft?.Invoke(Str(root, "player_id"), Str(root, "reason"));
                    break;
                case "host_changed":
                    HostChanged?.Invoke(Str(root, "player_id"));
                    break;
                case "host_reconnecting":
                    HostReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "host_reconnected":
                    HostReconnected?.Invoke(Str(root, "player_id"));
                    break;
                case "peer_reconnecting":
                    PeerReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "start_signaling":
                    StartSignaling?.Invoke(
                        Str(root, "gamemode"),
                        ReadWinCondition(root, "win_condition"),
                        ReadSpawnSettings(root, "spawn_settings"),
                        root.GetProperty("player_count").GetInt32(),
                        ReadStrings(root, "peers"),
                        ReadIceServers(root));
                    break;
                case "session_ended":
                    SessionEnded?.Invoke(Str(root, "reason"));
                    break;
                case "kicked":
                    Kicked?.Invoke(Str(root, "reason"));
                    break;
                case "score_update":
                    ScoreUpdate?.Invoke(ReadIntMap(root, "scores"));
                    break;
                case "game_over":
                    GameOver?.Invoke(Str(root, "winner_player_id"), ReadIntMap(root, "scores"));
                    break;
                case "chat_message":
                    // `kind` is absent on servers predating moderator chat; Str
                    // returns "" there, which reads as an ordinary player line.
                    // `body_id` is absent on servers predating deletion, and
                    // empty when the server could not record the line — either
                    // way the line renders and simply cannot be deleted.
                    ChatMessage?.Invoke(new ChatLine(
                        Str(root, "from"),
                        Str(root, "username"),
                        Str(root, "text"),
                        Str(root, "kind") == "moderator",
                        Str(root, "body_id")));
                    break;
                case "chat_warning":
                    ChatWarning?.Invoke(Str(root, "reason"));
                    break;
                case "chat_banned":
                    ChatBanned?.Invoke(Str(root, "reason"));
                    break;
                case "chat_body_deleted":
                    ChatBodyDeleted?.Invoke(Str(root, "body_id"));
                    break;
                case "match_started":
                    MatchStarted?.Invoke();
                    break;
                case "match_paused":
                    MatchPaused?.Invoke(
                        Str(root, "player_id"),
                        Str(root, "username"),
                        IntProp(root, "resume_timeout_secs"));
                    break;
                case "match_resumed":
                    MatchResumed?.Invoke(IntProp(root, "countdown_secs"));
                    break;
                case "time_sync":
                {
                    // T4 = now; fold this round-trip into the server-clock offset.
                    long t1 = LongProp(root, "client_send_ms");
                    long t4 = (long)Time.GetTicksMsec();
                    long serverMs = LongProp(root, "server_ms");
                    // TEMP diagnostic (fix/ball-handoff-entry-position): the entry
                    // fast-forward trusts this offset, so a biased one flings
                    // incoming balls inward. Log the RTT (a long/asymmetric path —
                    // e.g. a distant player to a US/EU server — biases the SNTP
                    // offset), THIS probe's raw sample, and how far it pulls from
                    // the running estimate: a distant client's samples jump around
                    // (big devFromEst), a close one's are tight. devFromEst is only
                    // meaningful once synced. Remove once confirmed.
                    long sampleOffset = serverMs - (t1 + (t4 - t1) / 2);
                    long prevEst = _clock.OffsetMs;
                    bool wasSynced = _clock.Synced;
                    _clock.AddSample(t1, serverMs, t4);
                    Log.Info("net.clock",
                        $"time_sync rtt={t4 - t1}ms sample={sampleOffset}ms " +
                        $"devFromEst={(wasSynced ? sampleOffset - prevEst : 0)}ms " +
                        $"smoothed={_clock.OffsetMs}ms synced={_clock.Synced}");
                    break;
                }
                case "offer":
                    OfferReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "answer":
                    AnswerReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "ice_candidate":
                    IceCandidateReceived?.Invoke(
                        Str(root, "from"),
                        Str(root, "candidate"),
                        Str(root, "sdp_mid"),
                        IntProp(root, "sdp_m_line_index"));
                    break;
                default:
                    Log.Debug("net.signaling", $"ignoring unknown frame type '{typeEl.GetString()}'");
                    break;
            }
        }
        catch (Exception e)
        {
            Log.Warn("net.signaling", $"failed to parse frame: {e.Message}");
        }
    }

    private static string Str(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) ? el.GetString() ?? "" : "";

    private static int IntProp(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.TryGetInt32(out var v) ? v : 0;

    private static long LongProp(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.TryGetInt64(out var v) ? v : 0;

    private static Dictionary<string, int> ReadIntMap(JsonElement obj, string name)
    {
        var map = new Dictionary<string, int>();
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
            foreach (var prop in el.EnumerateObject())
                if (prop.Value.TryGetInt32(out var v))
                    map[prop.Name] = v;
        return map;
    }

    /// <summary>Read a JSON object of string→string into a dictionary. Returns an
    /// empty map when the property is absent (so a client talking to a server
    /// that predates the field degrades gracefully rather than throwing).</summary>
    private static Dictionary<string, string> ReadStringMap(JsonElement obj, string name)
    {
        var map = new Dictionary<string, string>();
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
            foreach (var prop in el.EnumerateObject())
                if (prop.Value.ValueKind == JsonValueKind.String)
                    map[prop.Name] = prop.Value.GetString() ?? "";
        return map;
    }

    /// <summary>Read a <c>win_condition</c> object (<c>{kind,target}</c>) into a
    /// DTO. A missing/malformed field degrades to the default so a client talking
    /// to a server that predates the field still has a usable rule.</summary>
    private static WinConditionDto ReadWinCondition(JsonElement obj, string name)
    {
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
        {
            string kind = el.TryGetProperty("kind", out var k) && k.ValueKind == JsonValueKind.String
                ? k.GetString() ?? WinConditionDto.SetScoreKind
                : WinConditionDto.SetScoreKind;
            int target = el.TryGetProperty("target", out var t) && t.TryGetInt32(out var v)
                ? v
                : WinConditionDto.ScoreDefault;
            return new WinConditionDto(kind, target);
        }
        return WinConditionDto.Default;
    }

    /// <summary>Read a <c>spawn_settings</c> object into a DTO. A missing/malformed
    /// field degrades to the default so a client talking to a server that predates
    /// the field still has usable random-spawn rules.</summary>
    private static SpawnSettingsDto ReadSpawnSettings(JsonElement obj, string name)
    {
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
        {
            int interval = el.TryGetProperty("splitter_interval_secs", out var s) && s.TryGetInt32(out var v)
                ? v
                : SpawnSettingsDto.IntervalDefault;
            bool chain = el.TryGetProperty("chain_split", out var c)
                && (c.ValueKind == JsonValueKind.True || c.ValueKind == JsonValueKind.False)
                ? c.ValueKind == JsonValueKind.True
                : SpawnSettingsDto.ChainSplitDefault;
            return new SpawnSettingsDto(interval, chain);
        }
        return SpawnSettingsDto.Default;
    }

    /// <summary>Read the <c>ice_servers</c> array (server-minted STUN+TURN
    /// entries) into DTOs. Absent/malformed — an old server, TURN unconfigured,
    /// or a failed mint — degrades to an empty array; the transport then keeps
    /// its built-in STUN-only fallback. Entries without a usable <c>urls</c>
    /// array are skipped.</summary>
    private static IceServerDto[] ReadIceServers(JsonElement obj)
    {
        if (!obj.TryGetProperty("ice_servers", out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<IceServerDto>();
        var list = new List<IceServerDto>(arr.GetArrayLength());
        foreach (var item in arr.EnumerateArray())
        {
            if (item.ValueKind != JsonValueKind.Object)
                continue;
            var urls = ReadStrings(item, "urls");
            if (urls.Length == 0)
                continue;
            string? username = item.TryGetProperty("username", out var u) && u.ValueKind == JsonValueKind.String
                ? u.GetString()
                : null;
            string? credential = item.TryGetProperty("credential", out var c) && c.ValueKind == JsonValueKind.String
                ? c.GetString()
                : null;
            list.Add(new IceServerDto(urls, username, credential));
        }
        return list.ToArray();
    }

    private static string[] ReadStrings(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<string>();
        var list = new List<string>(arr.GetArrayLength());
        foreach (var item in arr.EnumerateArray())
            list.Add(item.GetString() ?? "");
        return list.ToArray();
    }
}
