using Godot;

namespace BriskaBlast.Net;

/// <summary>Server clock sync, probed over this same socket so ball handoffs can be
/// stamped in a shared time frame. The response half is handled with the other
/// frames; this is the probe cadence and the readings it feeds.</summary>
public partial class SignalingClient
{
    // Server clock sync. Periodically probes the server over this same WS so ball
    // handoffs can be stamped in a shared time frame (see ServerClock). The first
    // probe rides the open socket right after identify; thereafter every
    // SyncIntervalMsec, and immediately again after a reconnect (the local clock
    // may have stepped while we were away).
    private const ulong SyncIntervalMsec = 12_000;
    private readonly ServerClock _clock = new();
    private ulong _nextSyncMsec;

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

    /// <summary>Current time in the server-synced frame (ms). Both ends of a ball
    /// handoff stamp/compare with this so cross-machine wall-clock skew cancels
    /// and the transit fast-forward reflects only real network delay. Only
    /// meaningful once <see cref="ClockSynced"/> is true.</summary>
    public long ServerNowMs() => _clock.NowMs((long)Time.GetTicksMsec());

    /// <summary>Whether the server clock offset has at least one sample. Callers
    /// (the handoff fast-forward) should skip time-based correction until this is
    /// true rather than trust an unsynced, machine-local reading.</summary>
    public bool ClockSynced => _clock.Synced;
}
