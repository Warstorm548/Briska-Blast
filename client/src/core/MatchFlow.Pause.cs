namespace BriskaBlast.Core;

/// <summary>The server's mid-match pause relays, forwarded to the game view as
/// typed events. Only meaningful while InMatch: the rejoiner they announce is
/// itself in Preparing and enters via <c>match_started</c>.</summary>
public partial class MatchFlow
{
    /// <summary>Relay the server's pause to the game view with a display name.
    /// Only meaningful in-match: the rejoiner it announces is itself still in
    /// Preparing and enters via <c>match_started</c>, not this.</summary>
    private void OnMatchPaused(string playerId, string username, int resumeTimeoutSecs)
    {
        if (State != MatchFlowState.InMatch)
            return;
        var name = !string.IsNullOrEmpty(username)
            ? username
            : SessionContext.Instance.DisplayNameFor(playerId);
        Log.Info("match.flow", $"match paused for rejoining {playerId} (valve {resumeTimeoutSecs}s).");
        MatchPausedFor?.Invoke(name);
    }

    private void OnMatchResumed(int countdownSecs)
    {
        if (State != MatchFlowState.InMatch)
            return;
        Log.Info("match.flow", $"match resuming in {countdownSecs}s.");
        MatchResumedIn?.Invoke(countdownSecs);
    }
}
