using Godot;

namespace BriskaBlast.Game;

/// <summary>The BallSpliter spawn cadence. Each screen spawns its own locally; the
/// resulting split balls hand off across the mesh like any other ball.</summary>
public partial class GameScene
{
    /// <summary>Default seconds between BallSpliter spawns, used as a fallback until
    /// the host's setting is read (Stage 3). On average a splitter appears this often
    /// on each screen; the cooldown after one is consumed doubles as its respawn.</summary>
    private const double DefaultSplitterIntervalSecs = 15.0;

    // Random-spawn (BallSpliter) cadence. Each screen spawns its own splitters
    // locally on this cooldown; the resulting BallBT balls hand off like any other
    // ball. Stage 3 overrides the interval + chain-split from the host's settings.
    private double _splitterIntervalSecs = DefaultSplitterIntervalSecs;
    private double _splitterCooldown;

    /// <summary>Count down the splitter timer and drop a new one when it expires.
    /// No-ops when the host has splitters switched off. This timer owns the cadence,
    /// so consuming a splitter in the sim does not re-arm it.</summary>
    private void TickSplitters(double dt)
    {
        if (_splitterIntervalSecs <= 0)
            return; // disabled by the host

        _splitterCooldown -= dt;
        if (_splitterCooldown > 0)
            return;
        _splitterCooldown = _splitterIntervalSecs;

        // Drop a splitter at a random spot in the play area, clear of the very edges
        // and the paddle band. Consuming one in the sim doesn't re-arm — this timer
        // owns the cadence, so a fresh splitter follows roughly every interval.
        float margin = _ballRadius * 4f;
        float minX = margin, maxX = _state.ArenaWidth - margin;
        float minY = margin, maxY = _state.Paddle.Y - margin;
        if (maxX <= minX || maxY <= minY)
            return;

        float radius = _ballRadius * 1.5f;

        // Keep the splitter out of the corner barriers so it can't spawn unreachable.
        // Try a few random spots; if the corners crowd them all out this tick, skip the
        // spawn (the cadence timer already re-armed, so another follows next interval).
        for (int attempt = 0; attempt < 8; attempt++)
        {
            var pos = new Vector2(_rng.RandfRange(minX, maxX), _rng.RandfRange(minY, maxY));
            if (OverlapsBarrier(pos, radius))
                continue;
            _state.Splitters.Add(new Splitter
            {
                Id = _state.NextSplitterId(),
                Radius = radius,
                Pos = pos,
            });
            return;
        }
    }
}
