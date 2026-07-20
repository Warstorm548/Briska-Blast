using System;
using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Game;

/// <summary>A ball left this screen across a portal edge and must be handed to a
/// peer. Carries the exit in THIS player's local frame; the net layer
/// (<c>BallTransform</c>, Slice D) maps it onto the peer's entry edge.</summary>
public readonly struct HandoffEvent
{
    public readonly int BallId;
    public readonly string PeerId;
    public readonly Edge ExitEdge;
    /// <summary>Position along the exit edge in [0,1] (x/W for Top/Bottom,
    /// y/H for Left/Right).</summary>
    public readonly float NormalizedAlong;
    public readonly Vector2 Velocity;
    public readonly string LastHitterId;
    public readonly BallKind Kind;

    public HandoffEvent(int ballId, string peerId, Edge exitEdge, float along,
        Vector2 velocity, string lastHitterId, BallKind kind)
    {
        BallId = ballId;
        PeerId = peerId;
        ExitEdge = exitEdge;
        NormalizedAlong = along;
        Velocity = velocity;
        LastHitterId = lastHitterId;
        Kind = kind;
    }
}

/// <summary>A ball passed the local player's paddle into their goal.
/// <see cref="ScoringPlayerId"/> is the last hitter when a point is due, or empty
/// when none is — a self-goal (the scored-on player was the last hitter) or a ball
/// nobody had hit. <see cref="Points"/> is the goal's value (1 for the master ball,
/// 2 for a BallBT split ball). <see cref="Kind"/> is the goaled ball's kind: only a
/// lost <see cref="BallKind.Master"/> is re-served — a split ball just vanishes.</summary>
public readonly struct ScoreEvent
{
    public readonly string ScoringPlayerId;
    public readonly int Points;
    public readonly BallKind Kind;

    public ScoreEvent(string scoringPlayerId, int points, BallKind kind)
    {
        ScoringPlayerId = scoringPlayerId;
        Points = points;
        Kind = kind;
    }
}

/// <summary>Events emitted by one <see cref="GameSimulation.Step"/>. Owned and
/// reused by the caller (cleared at the start of each Step) to avoid allocating
/// in the physics loop.</summary>
public sealed class StepResult
{
    public readonly List<HandoffEvent> Handoffs = new();
    public readonly List<ScoreEvent> Scores = new();

    public void Clear()
    {
        Handoffs.Clear();
        Scores.Clear();
    }
}

/// <summary>
/// Authoritative, deterministic, node-free rules for one screen. Advances every
/// ball, reflects off wall edges + the paddle using trigonometric angles, and
/// reports portal crossings (handoffs) and goal crossings (scores) as events.
/// The simulation never renders and never touches the network.
/// </summary>
public static class GameSimulation
{
    /// <summary>Largest deflection from straight-up when the ball strikes the
    /// very edge of the paddle (radians). Centre hits go straight up.</summary>
    private const float MaxPaddleBounce = 1.0472f; // 60°

    /// <summary>Points a BallBT split ball is worth at a goal — double the master
    /// ball's single point. May change as the game evolves.</summary>
    private const int SplitBallPoints = 2;

    /// <summary>Number of BallBT split balls a splitter produces.</summary>
    private const int SplitBallCount = 3;

    /// <summary>Angle between adjacent split balls in the fan (45°).</summary>
    private static readonly float SplitFanSpacing = Mathf.Pi / 4f;

    /// <summary>The split fan is centred opposite the hitting ball's heading (180°),
    /// so the gap around the ball's forward path stays clear and the new balls don't
    /// immediately re-collide with it.</summary>
    private static readonly float SplitFanBaseOffset = Mathf.Pi;

    public static void Step(GameState state, double dt, StepResult result)
    {
        result.Clear();
        float dtf = (float)dt;

        // Iterate backwards so removing handed-off / scored balls is safe.
        for (int i = state.Balls.Count - 1; i >= 0; i--)
        {
            var ball = state.Balls[i];
            ball.Pos += ball.Vel * dtf;

            if (StepBall(state, ball, result))
                state.Balls.RemoveAt(i); // left the screen (handoff or goal)
        }

        // A ball touching a splitter spawns split balls — handled after integration
        // so the ball list is settled for this frame.
        ResolveSplitters(state);
    }

    /// <summary>Resolve one ball against the paddle and edges after integration.
    /// Returns true if the ball left this screen (caller removes it).</summary>
    private static bool StepBall(GameState state, Ball ball, StepResult result)
    {
        float w = state.ArenaWidth;
        float h = state.ArenaHeight;
        float r = ball.Radius;

        // --- Corner barriers: solid static obstacles in the four corners. Resolved
        // first so a ball entering a bottom goal corner bounces off instead of sneaking
        // past the paddle into the goal below. ---
        ResolveBarriers(state, ball);

        // --- Paddle (only while descending and overlapping the paddle face) ---
        var paddle = state.Paddle;
        if (ball.Vel.Y > 0 && ball.Pos.Y + r >= paddle.Y && ball.Pos.Y + r <= paddle.Y + paddle.Height)
        {
            float half = paddle.Width * 0.5f;
            float dx = ball.Pos.X - paddle.CenterX;
            if (Mathf.Abs(dx) <= half)
            {
                // Hit offset in [-1,1] steers the outgoing angle; speed preserved.
                float offset = Mathf.Clamp(dx / half, -1f, 1f);
                float angle = offset * MaxPaddleBounce;
                float speed = ball.Vel.Length();
                ball.Vel = new Vector2(Mathf.Sin(angle), -Mathf.Cos(angle)) * speed;
                ball.Pos = new Vector2(ball.Pos.X, paddle.Y - r);
                ball.LastHitterId = state.LocalPlayerId;
                return false;
            }
            // Missed horizontally — falls through toward the goal below.
        }

        // --- Goal: crossed the bottom line past the paddle ---
        if (ball.Pos.Y >= h)
        {
            // A point goes to the last hitter — unless that's the scored-on player
            // (a self-goal doesn't count) or nobody had hit it. A master ball is
            // worth 1 and is re-served by the scored-on player; a split (BallBT)
            // ball is worth 2 and just vanishes. The event fires either way so the
            // scorer is credited.
            bool counts = !string.IsNullOrEmpty(ball.LastHitterId) && ball.LastHitterId != state.LocalPlayerId;
            int points = ball.Kind == BallKind.Split ? SplitBallPoints : 1;
            result.Scores.Add(new ScoreEvent(counts ? ball.LastHitterId : "", points, ball.Kind));
            return true;
        }

        // --- Top / Left / Right edges: portal (handoff) or wall (reflect) ---
        if (ball.Vel.Y < 0 && ball.Pos.Y - r <= 0 &&
            ResolveEdge(state, ball, Edge.Top, ball.Pos.X / w, result))
            return true;

        if (ball.Vel.X < 0 && ball.Pos.X - r <= 0 &&
            ResolveEdge(state, ball, Edge.Left, ball.Pos.Y / h, result))
            return true;

        if (ball.Vel.X > 0 && ball.Pos.X + r >= w &&
            ResolveEdge(state, ball, Edge.Right, ball.Pos.Y / h, result))
            return true;

        return false;
    }

    /// <summary>Portal → emit a handoff (true = ball leaves). Wall → reflect the
    /// normal velocity component and clamp the ball back inside (false).</summary>
    private static bool ResolveEdge(GameState state, Ball ball, Edge edge, float along, StepResult result)
    {
        var target = state.Edges.TryGetValue(edge, out var t) ? t : EdgeTarget.Wall;

        if (target.Kind == EdgeKind.Portal)
        {
            result.Handoffs.Add(new HandoffEvent(
                ball.Id, target.PeerId, edge, Mathf.Clamp(along, 0f, 1f),
                ball.Vel, ball.LastHitterId, ball.Kind));
            return true;
        }

        // Wall (or an unexpected Goal on a non-bottom edge): trig reflection =
        // negate the component normal to this axis-aligned edge, reposition.
        float r = ball.Radius;
        switch (edge)
        {
            case Edge.Top:
                ball.Vel = new Vector2(ball.Vel.X, -ball.Vel.Y);
                ball.Pos = new Vector2(ball.Pos.X, r);
                break;
            case Edge.Left:
                ball.Vel = new Vector2(-ball.Vel.X, ball.Vel.Y);
                ball.Pos = new Vector2(r, ball.Pos.Y);
                break;
            case Edge.Right:
                ball.Vel = new Vector2(-ball.Vel.X, ball.Vel.Y);
                ball.Pos = new Vector2(state.ArenaWidth - r, ball.Pos.Y);
                break;
        }
        return false;
    }

    /// <summary>Bounce a ball off any corner barrier triangle it overlaps. Circle↔triangle:
    /// find the closest point on the triangle to the ball centre, and if it's within the
    /// radius, push the ball out along the surface normal and reflect its velocity across that
    /// normal (<c>v − 2(v·n)n</c>) — but only when it's moving into the surface, so a ball
    /// already leaving isn't yanked back (which would make it stick). On the diagonal
    /// hypotenuse this gives a true angled deflection, turning shots away from the goal corner.
    /// A centre that has fully penetrated the solid is pushed back out along its shallowest
    /// edge. Like the wall/goal checks this is position-based (no sweep), so a very fast ball
    /// could tunnel in one step — an accepted parity limitation. Barriers are simulated
    /// per-screen and never handed off across peers, so this float reflection needs no
    /// cross-peer determinism.</summary>
    private static void ResolveBarriers(GameState state, Ball ball)
    {
        var barriers = state.Barriers;
        float r = ball.Radius;
        for (int i = 0; i < barriers.Count; i++)
        {
            var t = barriers[i];
            var c = ball.Pos;
            Vector2 n;
            float push;

            if (CornerBarrier.PointInside(c, in t))
            {
                // Centre fully inside the solid: exit along the shallowest edge.
                (n, float depth) = CornerBarrier.NearestEdgeExit(c, in t);
                push = depth + r;
            }
            else
            {
                var p = CornerBarrier.ClosestPoint(c, in t);
                var d = c - p;
                float dist2 = d.LengthSquared();
                if (dist2 >= r * r)
                    continue; // circle doesn't reach this barrier

                float dist = Mathf.Sqrt(dist2);
                if (dist < 1e-5f)
                {
                    // Sitting on the boundary — fall back to the nearest-edge normal.
                    (n, float depth) = CornerBarrier.NearestEdgeExit(c, in t);
                    push = depth + r;
                }
                else
                {
                    n = d / dist;
                    push = r - dist;
                }
            }

            ball.Pos = c + n * push;
            float vn = ball.Vel.Dot(n);
            if (vn < 0f)
                ball.Vel -= 2f * vn * n;
        }
    }

    /// <summary>Resolve ball↔splitter collisions. A master ball always splits a
    /// splitter it overlaps; a split ball only does so when chain-splitting is on.
    /// The splitter is consumed (it disappears; the spawner re-arms the cooldown)
    /// and the hitting ball passes through unaffected.</summary>
    private static void ResolveSplitters(GameState state)
    {
        if (state.Splitters.Count == 0)
            return;

        // Backwards so a consumed splitter can be removed in place.
        for (int s = state.Splitters.Count - 1; s >= 0; s--)
        {
            var splitter = state.Splitters[s];
            for (int b = 0; b < state.Balls.Count; b++)
            {
                var ball = state.Balls[b];
                // Only the master always splits; split balls re-split only when the
                // host has chain-splitting enabled.
                if (ball.Kind == BallKind.Split && !state.ChainSplitEnabled)
                    continue;
                float reach = ball.Radius + splitter.Radius;
                if (ball.Pos.DistanceSquaredTo(splitter.Pos) > reach * reach)
                    continue;
                if (!SpawnSplitBalls(state, splitter, ball))
                    continue; // ball too slow to define a fan — leave the splitter

                state.Splitters.RemoveAt(s);
                break; // splitter consumed; on to the next one
            }
        }
    }

    /// <summary>Spawn the fan of BallBT split balls at the splitter's position,
    /// inheriting the hitting ball's owner and speed. Returns false (spawning
    /// nothing) if the hitting ball has no meaningful velocity to fan around.</summary>
    private static bool SpawnSplitBalls(GameState state, Splitter splitter, Ball hitter)
    {
        float speed = hitter.Vel.Length();
        if (speed <= 0.001f)
            return false;

        float heading = Mathf.Atan2(hitter.Vel.Y, hitter.Vel.X);
        float baseAngle = heading + SplitFanBaseOffset;
        // Centre the fan on baseAngle: for 3 balls → {−spacing, 0, +spacing}.
        float start = baseAngle - SplitFanSpacing * (SplitBallCount - 1) * 0.5f;
        for (int k = 0; k < SplitBallCount; k++)
        {
            float a = start + SplitFanSpacing * k;
            state.Balls.Add(new Ball
            {
                Id = state.NextBallId(),
                Pos = splitter.Pos,
                Vel = new Vector2(Mathf.Cos(a), Mathf.Sin(a)) * speed,
                Radius = hitter.Radius,
                Kind = BallKind.Split,
                LastHitterId = hitter.LastHitterId,
            });
        }
        return true;
    }
}
