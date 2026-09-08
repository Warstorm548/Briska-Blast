using Godot;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>The lobby safety net. <c>start_signaling</c> is a best-effort
/// broadcast, so a client whose socket was mid-reconnect at Start would sit in
/// the lobby forever; this poll spots the session leaving `waiting` and
/// converges through the rejoin sequence instead.</summary>
public partial class MatchFlow
{
    // ---- lobby safety-net poll (missed start_signaling) ----

    /// <summary>Lobby safety-net poll cadence. `start_signaling` is a
    /// best-effort broadcast — a client whose WS is mid-reconnect at Start
    /// misses it and would otherwise sit in the lobby forever; this poll spots
    /// the session leaving `waiting` and recovers. 7s keeps a full 4-player
    /// lobby behind one NAT at ~34 req/min, under the server's shared 60/min
    /// per-IP session limiter with headroom for real actions.</summary>
    private const ulong LobbyPollIntervalMsec = 7_000;
    private ulong _nextLobbyPollMsec;
    private bool _lobbyPollInFlight;

    /// <summary>Fire the next lobby poll when due. Best-effort: a failed poll
    /// just waits for the next interval — real session failures arrive as WS
    /// terminal events, never through here.</summary>
    private async void MaybePollLobby()
    {
        if (_lobbyPollInFlight || Time.GetTicksMsec() < _nextLobbyPollMsec)
            return;
        _lobbyPollInFlight = true;
        _nextLobbyPollMsec = Time.GetTicksMsec() + LobbyPollIntervalMsec;

        var ctx = SessionContext.Instance;
        var code = ctx.SessionCode;
        var result = await ctx.Api.GetSessionAsync(code);
        // Not on the main thread here — marshal back before touching state.
        Callable.From(() => OnLobbyPollResult(result, code)).CallDeferred();
    }

    private void OnLobbyPollResult(ApiResult<SessionPollResponse> result, string code)
    {
        _lobbyPollInFlight = false;
        // Only act if we're still in the same lobby the poll was sent from.
        if (State != MatchFlowState.InLobby || SessionContext.Instance.SessionCode != code)
            return;
        if (!result.Ok || result.Value is not { } info)
            return;
        if (info.Status is not ("starting" or "active"))
            return;

        RecoverMissedStart(info);
    }

    /// <summary>The session left `waiting` but no <c>start_signaling</c> ever
    /// arrived (our WS was mid-reconnect at Start). Converge through the rejoin
    /// sequence: adopt the rules the broadcast would have carried, replace the
    /// lobby socket with a fresh identify — whose <c>Identified</c> frame
    /// carries the frozen <c>seat_order</c> and the match's cached TURN
    /// credentials — and mesh behind the connecting screen.</summary>
    private void RecoverMissedStart(SessionPollResponse info)
    {
        Log.Warn("match.flow",
            $"lobby poll: session is '{info.Status}' but start_signaling never arrived — recovering.");

        var ctx = SessionContext.Instance;
        ctx.ApplyWinCondition(info.WinCondition);
        ctx.ApplySpawnSettings(info.SpawnSettings);
        ctx.ApplyLootSettings(info.LootSettings);

        // `active` means the barrier already resolved — a ball may be in play,
        // so the game scene must not serve (rejoin semantics). `starting` means
        // nobody is in-match yet; a recovered host still serves normally.
        IsRejoin = info.Status == "active";

        CloseSignaling(sendLeaveFrame: false);
        TransitionTo(MatchFlowState.Preparing, "recovered missed start");
        CarryChatIntoMatch();
        _prepareDeadlineMsec = Time.GetTicksMsec() + PrepareTimeoutMsec;
        EmitPreparing("Connecting to match…");

        if (GetTree().ChangeSceneToFile(PreparingScene) != Error.Ok)
        {
            FailFlow("Could not open the connecting screen.");
            return;
        }

        OpenSignaling(ctx);
    }
}
