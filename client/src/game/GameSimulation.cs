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

    public HandoffEvent(int ballId, string peerId, Edge exitEdge, float along,
        Vector2 velocity, string lastHitterId)
    {
        BallId = ballId;
        PeerId = peerId;
        ExitEdge = exitEdge;
        NormalizedAlong = along;
        Velocity = velocity;
        LastHitterId = lastHitterId;
    }
}

/// <summary>A ball passed the local player's paddle into their goal. Always
/// triggers a serve by the scored-on (local) player. <see cref="ScoringPlayerId"/>
/// is the last hitter when a point is due, or empty when none is — a self-goal
/// (the scored-on player was the last hitter) or a ball nobody had hit.</summary>
public readonly struct ScoreEvent
{
    public readonly string ScoringPlayerId;
    public ScoreEvent(string scoringPlayerId) => ScoringPlayerId = scoringPlayerId;
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
    }

    /// <summary>Resolve one ball against the paddle and edges after integration.
    /// Returns true if the ball left this screen (caller removes it).</summary>
    private static bool StepBall(GameState state, Ball ball, StepResult result)
    {
        float w = state.ArenaWidth;
        float h = state.ArenaHeight;
        float r = ball.Radius;

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
            // A point goes to the last hitter — unless that's the scored-on
            // player (a self-goal doesn't count) or nobody had hit it. Either
            // way the event fires so the scored-on player serves a replacement.
            bool counts = ball.LastHitterId.Length > 0 && ball.LastHitterId != state.LocalPlayerId;
            result.Scores.Add(new ScoreEvent(counts ? ball.LastHitterId : ""));
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
                ball.Vel, ball.LastHitterId));
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
}
