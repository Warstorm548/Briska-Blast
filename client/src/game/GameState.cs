using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// The four edges of a player's own screen, in the local (always-upright) frame.
/// <see cref="Bottom"/> is always this player's goal (the paddle sits above it);
/// the other three are portals to peers or solid walls. See
/// docs/planning/multiplayer-client-stages.md (Stage 3, Extended mode).
/// </summary>
public enum Edge
{
    Top,
    Right,
    Bottom,
    Left,
}

/// <summary>What lies across an edge.</summary>
public enum EdgeKind
{
    /// <summary>Solid boundary — the ball reflects (no peer occupies this slot).</summary>
    Wall,
    /// <summary>This player's goal line. The ball passing it (past the paddle)
    /// ends the rally and scores for the ball's last hitter.</summary>
    Goal,
    /// <summary>Shared edge with a peer — the ball crossing it is handed off to
    /// that peer (see <see cref="EdgeTarget.PeerId"/>).</summary>
    Portal,
}

/// <summary>What an edge connects to. <see cref="PeerId"/> is meaningful only
/// when <see cref="Kind"/> is <see cref="EdgeKind.Portal"/>.</summary>
public readonly struct EdgeTarget
{
    public readonly EdgeKind Kind;
    public readonly string PeerId;

    private EdgeTarget(EdgeKind kind, string peerId)
    {
        Kind = kind;
        PeerId = peerId;
    }

    public static readonly EdgeTarget Wall = new(EdgeKind.Wall, "");
    public static readonly EdgeTarget Goal = new(EdgeKind.Goal, "");
    public static EdgeTarget Portal(string peerId) => new(EdgeKind.Portal, peerId);
}

/// <summary>
/// A ball in flight on this screen. Plain data; the simulation integrates it and
/// the view draws it. <see cref="LastHitterId"/> travels with the ball across
/// handoffs so whoever's goal it eventually enters can credit the right scorer.
/// </summary>
public sealed class Ball
{
    public int Id;
    public Vector2 Pos;
    public Vector2 Vel;
    public float Radius = 16f;
    /// <summary>player_id of the last player to deflect this ball with a paddle;
    /// empty until someone has hit it.</summary>
    public string LastHitterId = "";
}

/// <summary>The local player's paddle: a horizontal bar that slides along the
/// bottom of the screen, a gap above the goal line.</summary>
public sealed class Paddle
{
    /// <summary>Center x of the paddle. Clamped to the arena by the simulation.</summary>
    public float CenterX;
    public float Width = 160f;
    public float Height = 24f;
    /// <summary>Top y of the paddle face the ball bounces off. Set from arena
    /// height minus the goal gap when the state is built.</summary>
    public float Y;
}

/// <summary>
/// All authoritative state for THIS player's screen. There is no shared arena:
/// each client owns and renders only its own <see cref="GameState"/>. The
/// simulation mutates it, the view observes it, and the net layer writes peer
/// handoffs / server score updates into it.
/// </summary>
public sealed class GameState
{
    public float ArenaWidth;
    public float ArenaHeight;

    public readonly Paddle Paddle = new();
    public readonly List<Ball> Balls = new();

    /// <summary>Per-edge mapping for this screen. Bottom is always
    /// <see cref="EdgeTarget.Goal"/>; Top/Right/Left are Portal or Wall.</summary>
    public readonly Dictionary<Edge, EdgeTarget> Edges = new();

    /// <summary>Mirror of the server-authoritative tally (player_id → points).
    /// Overwritten wholesale on each ScoreUpdate — never incremented locally.</summary>
    public readonly Dictionary<string, int> Scores = new();

    public string LocalPlayerId = "";

    private int _nextBallId;

    /// <summary>Monotonic ball id allocator (ids are unique within this screen's
    /// lifetime; handed-off balls keep the id assigned by their origin screen).</summary>
    public int NextBallId() => _nextBallId++;
}
