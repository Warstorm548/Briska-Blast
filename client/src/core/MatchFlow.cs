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
/// network. There is ONE start sequence, ONE rejoin sequence (both converge in
/// <see cref="MatchFlowState.Preparing"/>), and ONE teardown
/// (<see cref="LeaveSession"/>). Scenes are thin views: they render state,
/// call the entry points below, and subscribe to the typed events — only
/// pure-UI signaling events (chat, reconnect overlays, score paints) are
/// subscribed by views directly via <see cref="Signaling"/>.
/// </summary>
public partial class MatchFlow : Node
{
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

    /// <summary>Enter the lobby lifecycle: open the signaling socket for the
    /// current SessionContext session and move to InLobby. Called by the lobby
    /// scene's _Ready. (Wired in the scene-migration commit.)</summary>
    public void EnterLobby()
    {
        Log.Warn("match.flow", "EnterLobby called before wiring — no-op.");
    }

    /// <summary>Rejoin a live match (process-death recovery): open a fresh
    /// signaling socket, restore the frozen seating from its Identified frame,
    /// and converge into Preparing. Called by the Join screen after
    /// StartRejoinSession. (Wired in the scene-migration commit.)</summary>
    public void BeginRejoin()
    {
        Log.Warn("match.flow", "BeginRejoin called before wiring — no-op.");
    }

    /// <summary>Send a lobby chat line through the live signaling socket, so
    /// the lobby view never touches the socket to send.</summary>
    public void SendChat(string text)
    {
        Log.Warn("match.flow", "SendChat called before wiring — no-op.");
    }

    /// <summary>THE teardown: close and free the live net, clear the session,
    /// return to Idle and the main menu. <paramref name="sendLeaveFrame"/> sends
    /// a voluntary <c>leave</c> (frees the lobby slot immediately); false is a
    /// plain close — the server treats it as a transient drop and holds the slot
    /// for the reconnect grace, which mid-match exits rely on.</summary>
    public void LeaveSession(bool sendLeaveFrame)
    {
        Log.Warn("match.flow", "LeaveSession called before wiring — no-op.");
    }

    /// <summary>PostMatch navigation: same teardown as <see cref="LeaveSession"/>
    /// but landing on <paramref name="scenePath"/> (e.g. Host Setup for "Host
    /// Game") instead of the main menu.</summary>
    public void EndMatchTo(string scenePath)
    {
        Log.Warn("match.flow", "EndMatchTo called before wiring — no-op.");
    }

    /// <summary>Read-and-clear <see cref="LastFlowError"/>. The main menu calls
    /// this in _Ready to show why the player landed back there.</summary>
    public string TakeFlowError()
    {
        var msg = LastFlowError;
        LastFlowError = "";
        return msg;
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
