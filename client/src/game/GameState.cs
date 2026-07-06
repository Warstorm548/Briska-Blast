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

/// <summary>What kind of ball this is. The white master/starter ball is re-served
/// when it's lost and worth a single point; <see cref="Split"/> balls are the
/// BallBT bonus balls spawned by a ball splitter — worth double and simply removed
/// (never re-served) when they reach a goal. Drives texture, score weight and
/// re-serve behaviour.</summary>
public enum BallKind
{
    Master,
    Split,
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
    /// <summary>Master (the white starter ball) by default; <see cref="BallKind.Split"/>
    /// for BallBT balls produced by a splitter.</summary>
    public BallKind Kind = BallKind.Master;
}

/// <summary>A ball-splitter element (BallSpliter) sitting on this screen. A system
/// spawn: it appears on a host-configured cadence, and when a ball touches it the
/// splitter is consumed and spits out three BallBT split balls. Local to this
/// screen — splitters are never handed off; each client spawns its own.</summary>
public sealed class Splitter
{
    public int Id;
    public Vector2 Pos;
    public float Radius = 24f;
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

    /// <summary>System-spawned ball splitters currently on this screen (local-only;
    /// never networked). A ball touching one consumes it into three split balls.</summary>
    public readonly List<Splitter> Splitters = new();

    /// <summary>When true, BallBT split balls that hit another splitter split again;
    /// when false only the master ball can trigger a split. Host-configured.</summary>
    public bool ChainSplitEnabled = true;

    /// <summary>Solid corner-barrier collision triangles in arena pixels (one right triangle
    /// per corner, 4 total). Static local geometry like the wall edges — balls bounce off them
    /// but never hand off across them, so they're never networked. Built once in
    /// <c>GameScene._Ready</c> via <see cref="CornerBarrier.AppendTriangles"/>.</summary>
    public readonly List<CornerBarrier.BarrierTri> Barriers = new();

    /// <summary>Per-edge mapping for this screen. Bottom is always
    /// <see cref="EdgeTarget.Goal"/>; Top/Right/Left are Portal or Wall.</summary>
    public readonly Dictionary<Edge, EdgeTarget> Edges = new();

    /// <summary>Mirror of the server-authoritative tally (player_id → points).
    /// Overwritten wholesale on each ScoreUpdate — never incremented locally.</summary>
    public readonly Dictionary<string, int> Scores = new();

    public string LocalPlayerId = "";

    /// <summary>Per-seat stride that namespaces ball ids so balls created
    /// independently on different screens (split balls, simultaneous serves) never
    /// collide once handed across the mesh. Each screen's <see cref="BallIdBase"/>
    /// is its seat × this; a screen will never allocate near this many balls.</summary>
    public const int BallIdSeatStride = 1_000_000;

    /// <summary>Range of the per-process id offset folded into <see cref="BallIdBase"/>
    /// so a process-death rejoin (which restarts the local counter at 0) begins in a
    /// different sub-range of its seat block. Kept below the stride so the offset plus
    /// a match's ball count stays within the seat's block.</summary>
    public const int BallIdRejoinOffsetRange = 900_000;

    /// <summary>Start of this screen's ball-id block (seat × <see cref="BallIdSeatStride"/>),
    /// set once from the seating in GameScene. Defaults to 0 (seat 0 / unseated).</summary>
    public int BallIdBase;

    private int _nextBallId;

    /// <summary>Globally-unique ball id allocator. Monotonic within this screen's
    /// id block; handed-off balls keep the id assigned by their origin screen, so
    /// ids stay unique across the mesh.</summary>
    public int NextBallId() => BallIdBase + _nextBallId++;

    private int _nextSplitterId;

    /// <summary>Splitter id allocator. Splitters never leave this screen, so a plain
    /// local counter suffices (no cross-screen uniqueness needed).</summary>
    public int NextSplitterId() => _nextSplitterId++;
}
