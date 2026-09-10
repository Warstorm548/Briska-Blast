using Godot;
using System;
using System.Collections.Generic;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

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

    /// <summary>The match paused for a process-death rejoiner (server
    /// `match_paused`). Carries the rejoiner's display name for the "Waiting
    /// for {name}…" overlay. Fired only while InMatch — the rejoiner itself is
    /// in Preparing and its go-signal stays <c>match_started</c>.</summary>
    public event Action<string>? MatchPausedFor;

    /// <summary>The pause ended (server `match_resumed`). Carries the shared
    /// unfreeze countdown in seconds. Fired only while InMatch.</summary>
    public event Action<int>? MatchResumedIn;

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
        if (State == MatchFlowState.InLobby)
        {
            MaybePollLobby();
            return;
        }

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
