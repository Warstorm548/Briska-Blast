using Godot;
using System;
using System.Collections.Generic;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>Where the client is in the session lifecycle. Exactly one state at a
/// time; every change goes through <see cref="MatchFlow"/>'s transition gate.</summary>
public enum MatchFlowState
{
    /// <summary>No session. Main menu / setup screens.</summary>
    Idle,
    /// <summary>In a lobby with a live signaling socket, waiting for Start.</summary>
    InLobby,
    /// <summary>Match starting (or rejoining one): WebRTC mesh coming up behind
    /// the "Connecting to players…" screen. Ends in InMatch or a clean failure.</summary>
    Preparing,
    /// <summary>Playing. GameScene is up and the mesh carries handoffs.</summary>
    InMatch,
    /// <summary>Match over (GameOver received). The end-game screen owns
    /// navigation; the session teardown that follows is expected.</summary>
    PostMatch,
}

/// <summary>
/// The session lifecycle orchestrator (autoload singleton). Owns the live
/// <see cref="SignalingClient"/> and <see cref="IPeerTransport"/> as its own
/// children for their whole life — scenes never create, adopt, or tear down the
/// network. There is ONE start sequence (<c>start_signaling</c> → mesh), ONE
/// rejoin sequence (rejoin <c>Identified</c> → mesh — both converge in
/// <see cref="MatchFlowState.Preparing"/>), and ONE teardown
/// (<see cref="LeaveSession"/> / <see cref="EndMatchTo"/> /
/// <see cref="QuitGame"/>). Scenes are thin views: they render state, call the
/// entry points below, and subscribe to the typed events — only pure-UI
/// signaling events (chat, reconnect overlays, score paints) are subscribed by
/// views directly via <see cref="Signaling"/>.
/// </summary>
public partial class MatchFlow : Node
{
    private const string MainMenuScene = "res://src/ui/menus/MainMenu.tscn";
    private const string PreparingScene = "res://src/ui/menus/PreparingScreen.tscn";
    private const string GameScene = "res://src/game/GameScene.tscn";

    /// <summary>How long Preparing may take (signaling identify on a rejoin +
    /// full mesh bring-up + the server's ready barrier) before the attempt
    /// fails back to the main menu. Sized to the signaling reconnect window /
    /// server promotion grace, with room for TURN-relay ICE; the server's own
    /// barrier valve (20s) always resolves under this deadline, so waiting on
    /// <c>match_started</c> can only time out if the server is unreachable.</summary>
    private const ulong PrepareTimeoutMsec = 30_000;

    public static MatchFlow Instance { get; private set; } = null!;

    /// <summary>Current lifecycle state. Mutated only by the transition gate.</summary>
    public MatchFlowState State { get; private set; } = MatchFlowState.Idle;

    /// <summary>Live signaling connection for the current session; a child of
    /// this autoload, created fresh per session. Null while Idle.</summary>
    public SignalingClient? Signaling { get; private set; }

    /// <summary>Live peer transport for the current match; a child of this
    /// autoload. Null until Preparing begins.</summary>
    public IPeerTransport? Transport { get; private set; }

    /// <summary>True when the current Preparing/InMatch entered via a
    /// process-death rejoin (a ball is already in play elsewhere — the game
    /// scene must not serve). Cleared on teardown.</summary>
    public bool IsRejoin { get; private set; }

    /// <summary>Why the last session ended abnormally (prepare timeout, kick,
    /// terminal close, …). Set by the failure path, consumed once by the main
    /// menu via <see cref="TakeFlowError"/> so the player learns what happened.</summary>
    public string LastFlowError { get; private set; } = "";

    /// <summary>Latest Preparing status line; lets the Preparing screen paint
    /// the current progress on entry (events may have fired before its _Ready).</summary>
    public string PreparingStatus { get; private set; } = "";

    /// <summary>Fired after every accepted transition. Carries (from, to).</summary>
    public event Action<MatchFlowState, MatchFlowState>? StateChanged;

    /// <summary>Fired after a signaling event mutated the SessionContext roster
    /// (Identified / PeerJoined / PeerLeft / HostChanged) — views re-render.</summary>
    public event Action? RosterChanged;

    /// <summary>Human-readable Preparing progress for the connecting screen,
    /// e.g. "Connecting to players (1/3)…".</summary>
    public event Action<string>? PreparingProgress;

    /// <summary>The server declared the match over (GameOver relay). Carries
    /// (winnerPlayerId, finalScores). Fires alongside InMatch → PostMatch.</summary>
    public event Action<string, Dictionary<string, int>>? MatchEnded;

    // Preparing bookkeeping: the mesh is "up" when every expected peer's data
    // channel has opened. The transport has no aggregate signal — we count its
    // per-peer events against the roster we handed it.
    private readonly HashSet<string> _expectedPeers = new();
    private readonly HashSet<string> _connectedPeers = new();
    private readonly HashSet<string> _failedPeers = new();
    private ulong _prepareDeadlineMsec;

    // True once this client's mesh completed and its `client_ready` went out.
    // From then on Preparing waits solely on the server's `match_started` —
    // the only door into InMatch — and a WS blip re-sends the ready (it may
    // have been lost mid-reconnect; the server answers a duplicate directly).
    private bool _readySent;

    // Legal transitions. Anything else is logged and ignored — this table is
    // the single replacement for the per-scene one-shot guards (_leaving,
    // duplicate-start checks, late-SessionEnded checks).
    private static readonly Dictionary<MatchFlowState, MatchFlowState[]> Legal = new()
    {
        [MatchFlowState.Idle] = new[] { MatchFlowState.InLobby, MatchFlowState.Preparing },
        [MatchFlowState.InLobby] = new[] { MatchFlowState.Preparing, MatchFlowState.Idle },
        [MatchFlowState.Preparing] = new[] { MatchFlowState.InMatch, MatchFlowState.Idle },
        [MatchFlowState.InMatch] = new[] { MatchFlowState.PostMatch, MatchFlowState.Idle },
        [MatchFlowState.PostMatch] = new[] { MatchFlowState.Idle },
    };

    public override void _Ready()
    {
        Instance = this;
    }

    public override void _Process(double delta)
    {
        if (State != MatchFlowState.Preparing)
            return;

        // Fail fast once every expected peer has resolved and at least one
        // definitively failed — no point burning the rest of the deadline.
        bool allResolved = true;
        bool anyFailed = false;
        foreach (var p in _expectedPeers)
        {
            if (_failedPeers.Contains(p))
                anyFailed = true;
            else if (!_connectedPeers.Contains(p))
                allResolved = false;
        }
        if (allResolved && anyFailed)
        {
            FailFlow("Could not connect to all players.");
            return;
        }

        if (Time.GetTicksMsec() >= _prepareDeadlineMsec)
            FailFlow("Could not connect to all players (timed out).");
    }

    // ---- entry points (called by scenes) ----

    /// <summary>Enter the lobby lifecycle: open a fresh signaling socket for the
    /// session already seeded into <see cref="SessionContext"/> (by /host or
    /// /join) and move to InLobby. Called by the lobby scene's _Ready.</summary>
    public void EnterLobby()
    {
        if (State != MatchFlowState.Idle)
        {
            // Defensive: a stale session should never survive into a new lobby,
            // but if one did, drop it rather than leak two live sockets.
            Log.Warn("match.flow", $"EnterLobby while {State} — tearing stale session down first.");
            Teardown(sendLeaveFrame: false, why: "stale session on EnterLobby");
        }

        // Transition BEFORE dialing: SignalingClient.Connect emits Closed
        // synchronously when the dial itself fails, and OnClosed ignores closes
        // while Idle — so the state must already be InLobby for an immediate
        // connect failure to fail the flow instead of being swallowed.
        TransitionTo(MatchFlowState.InLobby, "entered lobby");
        OpenSignaling(SessionContext.Instance);
    }

    /// <summary>Rejoin a live match (process-death recovery): open a fresh
    /// signaling socket and converge into Preparing behind the connecting
    /// screen. The frozen seating and the match's TURN credentials arrive in
    /// the <c>Identified</c> frame; the mesh comes up from there. Called by the
    /// Join screen after <see cref="SessionContext.StartRejoinSession"/>.</summary>
    public void BeginRejoin()
    {
        if (State != MatchFlowState.Idle)
        {
            Log.Warn("match.flow", $"BeginRejoin while {State} — tearing stale session down first.");
            Teardown(sendLeaveFrame: false, why: "stale session on BeginRejoin");
        }

        IsRejoin = true;
        // Transition + connecting screen BEFORE dialing (same reasoning as
        // EnterLobby): a synchronous connect failure then lands in OnClosed
        // with the state already Preparing, and its teardown's main-menu scene
        // change — issued last — wins over the connecting screen.
        TransitionTo(MatchFlowState.Preparing, "rejoining live match");

        // The deadline covers identify + mesh together — a rejoin that can't
        // produce a working mesh within the window fails back cleanly.
        _prepareDeadlineMsec = Time.GetTicksMsec() + PrepareTimeoutMsec;
        EmitPreparing("Rejoining match…");

        if (GetTree().ChangeSceneToFile(PreparingScene) != Error.Ok)
        {
            FailFlow("Could not open the connecting screen.");
            return;
        }

        OpenSignaling(SessionContext.Instance);
    }

    /// <summary>Send a lobby chat line through the live signaling socket, so
    /// the lobby view never touches the socket to send.</summary>
    public void SendChat(string text) => Signaling?.SendChatMessage(text);

    /// <summary>THE teardown: close and free the live net, clear the session,
    /// return to Idle and the main menu. <paramref name="sendLeaveFrame"/> sends
    /// a voluntary <c>leave</c> (frees the lobby slot immediately); false is a
    /// plain close — the server treats it as a transient drop and holds the slot
    /// for the reconnect grace, which mid-match exits rely on.</summary>
    public void LeaveSession(bool sendLeaveFrame)
    {
        if (State == MatchFlowState.Idle)
            return;
        Teardown(sendLeaveFrame, "leave session");
        if (GetTree().ChangeSceneToFile(MainMenuScene) != Error.Ok)
            GD.PushError("[match.flow] failed to change to the main menu.");
    }

    /// <summary>Tear the session down and land on <paramref name="scenePath"/>
    /// instead of the main menu (e.g. Host Setup for "Host Game" after a win, or
    /// the lobby host's "Return to Setup"). Same single teardown underneath.</summary>
    public void EndMatchTo(string scenePath)
    {
        if (State == MatchFlowState.Idle)
            return;
        Teardown(sendLeaveFrame: false, why: $"end match → {scenePath}");
        if (GetTree().ChangeSceneToFile(scenePath) != Error.Ok)
        {
            GD.PushError($"[match.flow] failed to change to {scenePath} — falling back to menu.");
            GetTree().ChangeSceneToFile(MainMenuScene);
        }
    }

    /// <summary>Tear the session down (transient-drop semantics: peers keep our
    /// slot and run the grace timers) and close the application.</summary>
    public void QuitGame()
    {
        Teardown(sendLeaveFrame: false, why: "quit game");
        GetTree().Quit();
    }

    /// <summary>Read-and-clear <see cref="LastFlowError"/>. The main menu calls
    /// this in _Ready to show why the player landed back there.</summary>
    public string TakeFlowError()
    {
        var msg = LastFlowError;
        LastFlowError = "";
        return msg;
    }

    // ---- the two sequences, converging in Preparing ----

    /// <summary>Create and connect the per-session signaling socket (a fresh
    /// node every session) and install this orchestrator as its sole
    /// lifecycle-event subscriber.</summary>
    private void OpenSignaling(SessionContext ctx)
    {
        var s = new SignalingClient();
        AddChild(s);
        s.Identified += OnIdentified;
        s.PeerJoined += OnPeerJoined;
        s.PeerLeft += OnPeerLeft;
        s.HostChanged += OnHostChanged;
        s.StartSignaling += OnStartSignaling;
        s.SessionEnded += OnSessionEnded;
        s.Kicked += OnKicked;
        s.Closed += OnClosed;
        s.GameOver += OnGameOver;
        s.MatchStarted += OnMatchStarted;
        s.Reconnected += OnReconnected;
        Signaling = s;
        s.Connect(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
    }

    /// <summary>Handle <c>start_signaling</c> (fresh start, host and joiners
    /// alike): adopt the authoritative match rules, freeze the seating roster,
    /// and bring the mesh up behind the connecting screen.</summary>
    private void OnStartSignaling(string gamemode, WinConditionDto winCondition,
        SpawnSettingsDto spawnSettings, int playerCount, string[] peers,
        IceServerDto[] iceServers)
    {
        // The transition gate rejects duplicates / late frames (only InLobby
        // may start), replacing the old `_transport != null` one-shot guard.
        if (!TransitionTo(MatchFlowState.Preparing, "start_signaling"))
            return;

        var ctx = SessionContext.Instance;
        ctx.ApplyWinCondition(winCondition);
        ctx.ApplySpawnSettings(spawnSettings);
        // `peers` is the server's authoritative, self-inclusive start-time
        // roster ([host, …joiners] in join order) — identical on every client —
        // frozen here for the Extended-mode portal layout (GameScene.BuildEdges).
        ctx.SetSeatOrder(peers);

        _prepareDeadlineMsec = Time.GetTicksMsec() + PrepareTimeoutMsec;

        var expected = new List<string>(peers.Length);
        foreach (var p in peers)
            if (p != ctx.PlayerId)
                expected.Add(p);
        StartMesh(ctx, expected, iceServers);

        if (State == MatchFlowState.Preparing &&
            GetTree().ChangeSceneToFile(PreparingScene) != Error.Ok)
            FailFlow("Could not open the connecting screen.");
    }

    /// <summary>Handle an <c>Identified</c> frame. In the lobby (and on a
    /// mid-match re-identify) it refreshes the roster snapshot. On a rejoin it
    /// is the moment the frozen seating + TURN credentials arrive — the rejoin
    /// path's equivalent of <c>start_signaling</c> — so the mesh starts here.</summary>
    private void OnIdentified(string hostId, string[] peers, string[] seatOrder,
        bool isHost, Dictionary<string, string> usernames, IceServerDto[] iceServers)
    {
        var ctx = SessionContext.Instance;
        ctx.MergeUsernames(usernames);
        ctx.HostPlayerId = hostId;
        RebuildRoster(ctx, hostId, peers);

        // Rejoin flavor, first identify only (Transport not yet built): restore
        // the frozen seating the match started with and re-mesh to the current
        // members. seat_order is non-empty for any started match; empty means
        // the server predates it or the session isn't actually live — fail
        // rather than lay out portals from nothing.
        if (State == MatchFlowState.Preparing && IsRejoin && Transport == null)
        {
            if (seatOrder.Length == 0)
            {
                FailFlow("Could not rejoin the match.");
                return;
            }
            ctx.SetSeatOrder(seatOrder);
            StartMesh(ctx, peers, iceServers);
        }

        RosterChanged?.Invoke();
    }

    /// <summary>Shared mesh bring-up — the single successor to the duplicated
    /// lobby-start / rejoin choreographies. Creates the transport, applies the
    /// match's ICE servers (must precede Connect — the config is baked into
    /// each peer connection at creation), and starts counting channels.</summary>
    private void StartMesh(SessionContext ctx, IReadOnlyList<string> expectedPeers,
        IceServerDto[] iceServers)
    {
        _expectedPeers.Clear();
        _connectedPeers.Clear();
        _failedPeers.Clear();
        _readySent = false;
        foreach (var p in expectedPeers)
            if (p != ctx.PlayerId)
                _expectedPeers.Add(p);

        var transport = new WebRtcMeshTransport();
        transport.Init(Signaling!);
        transport.SetIceServers(iceServers);
        AddChild(transport);
        transport.PeerConnected += OnMeshPeerConnected;
        transport.PeerFailed += OnMeshPeerFailed;
        transport.PeerDisconnected += OnMeshPeerDisconnected;
        Transport = transport;
        transport.Connect(ctx.PlayerId, new List<string>(_expectedPeers));

        EmitPreparing($"Connecting to players (0/{_expectedPeers.Count})…");
        CheckPreparingComplete(); // zero expected peers → straight in
    }

    private void OnMeshPeerConnected(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _connectedPeers.Add(peerId);
        _failedPeers.Remove(peerId);
        int have = 0;
        foreach (var p in _expectedPeers)
            if (_connectedPeers.Contains(p))
                have++;
        EmitPreparing($"Connecting to players ({have}/{_expectedPeers.Count})…");
        CheckPreparingComplete();
    }

    private void OnMeshPeerFailed(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _failedPeers.Add(peerId);
        _connectedPeers.Remove(peerId);
    }

    private void OnMeshPeerDisconnected(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _connectedPeers.Remove(peerId);
    }

    private void CheckPreparingComplete()
    {
        if (State != MatchFlowState.Preparing)
            return;
        if (!_expectedPeers.IsSubsetOf(_connectedPeers))
            return;

        // Mesh up ≠ match on: everyone else's mesh must be up too. Report
        // ready and hold for the server's match_started (broadcast when all
        // are ready or its 20s valve fires — always under our deadline).
        if (_readySent)
            return;
        _readySent = true;
        Log.Info("match.flow", "mesh complete — client_ready sent, waiting for match_started.");
        Signaling?.SendClientReady();
        EmitPreparing("Waiting for other players…");
    }

    /// <summary>The server's <c>match_started</c> — the ready barrier resolved
    /// (or a direct reply to a straggler's ready). The only door into InMatch.</summary>
    private void OnMatchStarted()
    {
        // If the barrier's valve started the match while our own mesh is still
        // coming up, stay in Preparing: our ready will be answered directly
        // with another match_started once the mesh completes.
        if (State != MatchFlowState.Preparing || !_readySent)
        {
            Log.Debug("match.flow", $"match_started ignored (state {State}, readySent {_readySent}).");
            return;
        }

        if (!TransitionTo(MatchFlowState.InMatch, "match started"))
            return;
        if (GetTree().ChangeSceneToFile(GameScene) != Error.Ok)
        {
            // Revert is impossible mid-flight — fail through the one path.
            Log.Error("match.flow", "failed to enter GameScene.");
            LastFlowError = "Could not start the match.";
            LeaveSession(sendLeaveFrame: false);
        }
    }

    /// <summary>A WS blip healed mid-Preparing: the ready we sent may have been
    /// lost with the old socket, so send it again — the server answers a
    /// duplicate (or post-start) ready with a direct <c>match_started</c>.</summary>
    private void OnReconnected()
    {
        if (State == MatchFlowState.Preparing && _readySent)
        {
            Log.Info("match.flow", "reconnected during Preparing — re-sending client_ready.");
            Signaling?.SendClientReady();
        }
    }

    // ---- roster events (SessionContext is mutated only here) ----

    /// <summary>Rebuild the display roster from an Identified snapshot: host
    /// first, then self, then the remaining peers — the ordering the lobby
    /// slots render. (Moved verbatim from the lobby/rejoin duplicates.)</summary>
    private static void RebuildRoster(SessionContext ctx, string hostId, string[] peers)
    {
        ctx.PlayerIds.Clear();
        if (!string.IsNullOrEmpty(hostId))
            ctx.PlayerIds.Add(hostId);
        if (ctx.PlayerId != hostId)
            ctx.PlayerIds.Add(ctx.PlayerId);
        foreach (var p in peers)
            if (p != hostId && p != ctx.PlayerId && !ctx.PlayerIds.Contains(p))
                ctx.PlayerIds.Add(p);
    }

    private void OnPeerJoined(string playerId, string username)
    {
        var ctx = SessionContext.Instance;
        ctx.SetUsername(playerId, username);
        if (!ctx.PlayerIds.Contains(playerId))
            ctx.PlayerIds.Add(playerId);
        RosterChanged?.Invoke();
    }

    private void OnPeerLeft(string playerId, string reason)
    {
        SessionContext.Instance.PlayerIds.Remove(playerId);
        // A member who left while we're still meshing is no longer expected —
        // don't hang Preparing waiting on a ghost.
        if (State == MatchFlowState.Preparing && _expectedPeers.Remove(playerId))
        {
            _connectedPeers.Remove(playerId);
            _failedPeers.Remove(playerId);
            CheckPreparingComplete();
        }
        RosterChanged?.Invoke();
    }

    private void OnHostChanged(string playerId)
    {
        SessionContext.Instance.HostPlayerId = playerId;
        RosterChanged?.Invoke();
    }

    // ---- terminal events ----

    private void OnSessionEnded(string reason)
    {
        // After a win the server hands cleanup to SessionEnded right behind the
        // GameOver frame; the end screen owns navigation, so this one is expected.
        if (State == MatchFlowState.PostMatch || State == MatchFlowState.Idle)
            return;
        FailFlow($"Session ended ({reason}).");
    }

    private void OnKicked(string reason) => FailFlow($"Removed from session ({reason}).");

    private void OnClosed(int code, string reason)
    {
        if (State == MatchFlowState.Idle || State == MatchFlowState.PostMatch)
            return;
        // 1000 is a normal close; 4403/4404 during a rejoin attempt get the
        // friendly rejoin wording; anything else is an unexpected loss.
        string msg = code switch
        {
            4403 when IsRejoin => "You're not part of that match.",
            4404 when IsRejoin => "That match no longer exists.",
            1000 => "Disconnected from session.",
            _ => $"Connection closed ({code}).",
        };
        FailFlow(msg);
    }

    private void OnGameOver(string winnerPlayerId, Dictionary<string, int> scores)
    {
        if (State == MatchFlowState.Preparing)
        {
            // Rejoined into a match that ended while we were connecting.
            FailFlow("The match just ended.");
            return;
        }
        if (!TransitionTo(MatchFlowState.PostMatch, "game over"))
            return;
        MatchEnded?.Invoke(winnerPlayerId, scores);
    }

    // ---- the one failure + teardown path ----

    /// <summary>Abnormal end: remember why (for the main menu), then run the
    /// one teardown back to the menu.</summary>
    private void FailFlow(string message)
    {
        Log.Info("match.flow", $"flow failed: {message}");
        LastFlowError = message;
        LeaveSession(sendLeaveFrame: false);
    }

    /// <summary>Close and free the live net, clear the session, return to Idle.
    /// Every exit — voluntary, failure, quit — funnels through here.</summary>
    private void Teardown(bool sendLeaveFrame, string why)
    {
        if (Signaling is { } s)
        {
            s.Identified -= OnIdentified;
            s.PeerJoined -= OnPeerJoined;
            s.PeerLeft -= OnPeerLeft;
            s.HostChanged -= OnHostChanged;
            s.StartSignaling -= OnStartSignaling;
            s.SessionEnded -= OnSessionEnded;
            s.Kicked -= OnKicked;
            s.Closed -= OnClosed;
            s.GameOver -= OnGameOver;
            s.MatchStarted -= OnMatchStarted;
            s.Reconnected -= OnReconnected;
            if (sendLeaveFrame)
                s.SendLeave();
        }

        if (Transport is { } t)
        {
            t.PeerConnected -= OnMeshPeerConnected;
            t.PeerFailed -= OnMeshPeerFailed;
            t.PeerDisconnected -= OnMeshPeerDisconnected;
            t.Close();
            if (t is Node tn)
                tn.QueueFree();
        }
        Transport = null;

        Signaling?.CloseConnection();
        Signaling?.QueueFree();
        Signaling = null;

        SessionContext.Instance?.ClearSession();
        IsRejoin = false;
        PreparingStatus = "";
        _expectedPeers.Clear();
        _connectedPeers.Clear();
        _failedPeers.Clear();
        _readySent = false;
        TransitionTo(MatchFlowState.Idle, why);
    }

    private void EmitPreparing(string status)
    {
        PreparingStatus = status;
        PreparingProgress?.Invoke(status);
    }

    /// <summary>The single transition gate. Accepted transitions are logged and
    /// fire <see cref="StateChanged"/>; anything else is logged and ignored so a
    /// duplicate or late frame can never derail the lifecycle.</summary>
    private bool TransitionTo(MatchFlowState to, string why)
    {
        if (State == to || Array.IndexOf(Legal[State], to) < 0)
        {
            Log.Warn("match.flow", $"ignored illegal transition {State} -> {to} ({why})");
            return false;
        }
        var from = State;
        State = to;
        Log.Info("match.flow", $"state {from} -> {to} ({why})");
        StateChanged?.Invoke(from, to);
        return true;
    }
}
