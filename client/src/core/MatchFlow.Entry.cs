using Godot;

namespace BriskaBlast.Core;

/// <summary>The entry points scenes call: the two ways into a session and the
/// three ways out. Every exit funnels into the one <c>Teardown</c> in
/// MatchFlow.Terminal.cs.</summary>
public partial class MatchFlow
{
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
        // First safety-net poll one interval out — the WS broadcast is the
        // normal path; the poll only exists to catch a missed one.
        _nextLobbyPollMsec = Time.GetTicksMsec() + LobbyPollIntervalMsec;
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
        // A fresh process has nothing to carry; the call logs the zero and the
        // rejoiner starts from an empty transcript by design.
        CarryChatIntoMatch();

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
}
