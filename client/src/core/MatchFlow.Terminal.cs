using Godot;
using System.Collections.Generic;

namespace BriskaBlast.Core;

/// <summary>Every way a session ends, and the single teardown they all reach.
/// Closing the socket is factored out into MatchFlow.Signaling.cs because the
/// missed-start recovery reuses it without ending the session.</summary>
public partial class MatchFlow
{
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
        CloseSignaling(sendLeaveFrame);

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

        SessionContext.Instance?.ClearSession();
        IsRejoin = false;
        PreparingStatus = "";
        // Deliberately here and not in CloseSignaling: the missed-start recovery
        // closes and reopens the socket without ending the session, and the
        // conversation has to survive that swap.
        Chat.Clear();
        _expectedPeers.Clear();
        _connectedPeers.Clear();
        _failedPeers.Clear();
        _readySent = false;
        TransitionTo(MatchFlowState.Idle, why);
    }
}
