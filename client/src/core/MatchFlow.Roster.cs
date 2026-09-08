namespace BriskaBlast.Core;

/// <summary>The roster events. <see cref="SessionContext"/>'s membership is
/// mutated here and nowhere else; views re-render off <c>RosterChanged</c>.</summary>
public partial class MatchFlow
{
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
}
