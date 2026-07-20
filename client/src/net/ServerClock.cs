using System;

namespace BriskaBlast.Net;

/// <summary>
/// Estimates this client's offset to the <em>server's</em> clock so every peer
/// can express times in one shared frame. Pure logic — no Godot, no I/O — driven
/// by <see cref="SignalingClient"/>, which feeds it round-trip samples and reads
/// <see cref="NowMs"/> back. Isolated (like <c>BallTransform</c> /
/// <c>GameSimulation</c>) so the timing math is reviewable on its own.
///
/// <para>Why it exists: the Extended-mode ball handoff fast-forwards an entering
/// ball by its transit time. That transit was once measured as the difference of
/// two <em>independent machine wall clocks</em> (sender vs receiver), so any skew
/// between the PCs leaked straight into the ball's entry position — and grew as
/// the clocks drifted, eventually flinging the ball partway down one player's
/// screen. Stamping the handoff in server-frame time instead makes the skew term
/// cancel: <c>transit</c> then reflects only real network delay.</para>
///
/// <para>Estimation is SNTP-style. For a probe sent at local time <c>T1</c>,
/// answered with the server's <c>serverMs</c>, and received at local <c>T4</c>:
/// <c>offset = serverMs − (T1 + T4) / 2</c> (assuming a symmetric path), and the
/// round-trip delay is <c>T4 − T1</c>. <c>T1</c>/<c>T4</c> come from a
/// <b>monotonic</b> source (the caller's <c>Time.GetTicksMsec()</c>) so a local
/// wall-clock step can't corrupt a measurement; the per-machine tick baseline is
/// absorbed into the offset.</para>
/// </summary>
public sealed class ServerClock
{
    // Samples whose round-trip exceeds this are dropped: a congested probe gives
    // a poor (asymmetric) offset estimate. Generous enough that normal LAN/WAN
    // RTTs (and a future TURN-relayed path) still pass.
    private const long MaxRttMs = 1_000;

    // Weight of each accepted sample once seeded, so a single noisy probe can't
    // jerk the offset. The first sample seeds directly (no history to blend).
    private const double SampleWeight = 0.25;

    private long _offsetMs;
    private bool _synced;

    /// <summary>True once at least one usable sample has been folded in;
    /// <see cref="NowMs"/> is only meaningful while this holds.</summary>
    public bool Synced => _synced;

    /// <summary>Fold one round-trip into the offset estimate. All three times are
    /// in the SAME monotonic frame except <paramref name="serverMs"/>, which is
    /// the server's wall clock from the reply. Out-of-order or congested samples
    /// (negative or over-large RTT) are ignored.</summary>
    /// <param name="t1Ticks">Local monotonic time the probe was sent.</param>
    /// <param name="serverMs">Server wall-clock time carried in the reply.</param>
    /// <param name="t4Ticks">Local monotonic time the reply was received.</param>
    public void AddSample(long t1Ticks, long serverMs, long t4Ticks)
    {
        long rtt = t4Ticks - t1Ticks;
        if (rtt < 0 || rtt > MaxRttMs)
            return;

        // Midpoint of send/receive is our best guess for "local time when the
        // server stamped serverMs"; the offset carries us from local → server.
        // Written as t1 + rtt/2 (rtt is already non-negative here) rather than
        // (t1 + t4)/2 so the intermediate sum can't overflow.
        long midpoint = t1Ticks + rtt / 2;
        long sampleOffset = serverMs - midpoint;

        if (!_synced)
        {
            _offsetMs = sampleOffset;
            _synced = true;
        }
        else
        {
            _offsetMs = (long)Math.Round(
                _offsetMs * (1.0 - SampleWeight) + sampleOffset * SampleWeight);
        }
    }

    /// <summary>Server-frame time for a given local monotonic reading. Caller
    /// passes its current <c>Time.GetTicksMsec()</c>. Before the first sync this
    /// is just the raw local reading (offset 0) — callers should gate on
    /// <see cref="Synced"/> rather than trust it cross-machine.</summary>
    public long NowMs(long localTicks) => localTicks + _offsetMs;

    /// <summary>Diagnostic read of the current offset estimate (server − local, ms).
    /// Exposed so callers can log clock health; not part of normal operation.</summary>
    public long OffsetMs => _offsetMs;
}
