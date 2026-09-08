using Godot;
using System.Collections.Generic;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>The two ways a match begins — a fresh <c>start_signaling</c> and a
/// rejoin's <c>Identified</c> — converging in Preparing, plus the server's
/// <c>match_started</c> that is the only door into InMatch.</summary>
public partial class MatchFlow
{
    /// <summary>Handle <c>start_signaling</c> (fresh start, host and joiners
    /// alike): adopt the authoritative match rules, freeze the seating roster,
    /// and bring the mesh up behind the connecting screen.</summary>
    private void OnStartSignaling(string gamemode, WinConditionDto winCondition,
        SpawnSettingsDto spawnSettings, LootSettingsDto lootSettings, int playerCount,
        string[] peers, IceServerDto[] iceServers)
    {
        // The transition gate rejects duplicates / late frames (only InLobby
        // may start), replacing the old `_transport != null` one-shot guard.
        if (!TransitionTo(MatchFlowState.Preparing, "start_signaling"))
            return;

        // The lobby's conversation becomes the match's, capped. From here the
        // transcript keeps recording with no view mounted, so a line sent while
        // players stare at the connecting screen still lands in the match log.
        CarryChatIntoMatch();

        var ctx = SessionContext.Instance;
        ctx.ApplyWinCondition(winCondition);
        ctx.ApplySpawnSettings(spawnSettings);
        ctx.ApplyLootSettings(lootSettings);
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

        // First identify of a Preparing entered without start_signaling
        // (Transport not yet built): a process-death rejoin, or the lobby
        // poll's missed-start recovery. Restore the frozen seating the match
        // started with and mesh to the current members. The normal start path
        // never lands here — OnStartSignaling builds the transport before any
        // later identify. seat_order is non-empty for any started match; empty
        // means the server predates it or the session isn't actually live —
        // fail rather than lay out portals from nothing.
        if (State == MatchFlowState.Preparing && Transport == null)
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
        // In-match now: a later transient WS blip must re-identify as a normal
        // member — only the not-yet-meshed rejoin identifies may pause the match.
        if (Signaling != null)
            Signaling.IdentifyAsRejoin = false;
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
}
