using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game.View;
using BriskaBlast.Net;

namespace BriskaBlast.Game;

/// <summary>
/// Runs one player's screen of an Extended-mode round. Owns the authoritative
/// <see cref="GameState"/> for this screen, steps <see cref="GameSimulation"/>
/// each physics frame, and drives a <see cref="View2D"/>. Entered from the lobby
/// once the WebRTC mesh is up (never standalone). The ball handoff over the
/// transport and the score report to the server are wired in Slice D — marked
/// below.
/// </summary>
public partial class GameScene : Node2D
{
    // Tuning placeholders — gameplay feel (speeds, sizes, gap) is adjusted later.
    private const float PaddleSpeed = 1400f;
    private const float ServeSpeed = 900f;
    private const float GoalGap = 120f;
    private const float PaddleWidth = 240f;
    private const float PaddleHeight = 36f;
    private const float BallRadius = 24f;

    // Peers fill these edges (bottom is always the local goal).
    private static readonly Edge[] PortalSlots = { Edge.Top, Edge.Right, Edge.Left };

    private GameState _state = null!;
    private View2D _view = null!;
    private readonly StepResult _step = new();

    private bool _awaitingServe;
    private Ball? _serveBall;

    private SignalingClient? _signaling;
    private NetGameController? _controller;
    private bool _leaving;

    public override void _Ready()
    {
        var ctx = SessionContext.Instance;
        var arena = GetViewportRect().Size;

        _state = new GameState
        {
            ArenaWidth = arena.X,
            ArenaHeight = arena.Y,
            LocalPlayerId = ctx?.PlayerId ?? "",
        };
        _state.Paddle.Width = PaddleWidth;
        _state.Paddle.Height = PaddleHeight;
        _state.Paddle.CenterX = arena.X * 0.5f;
        _state.Paddle.Y = arena.Y - GoalGap;

        BuildEdges(ctx);

        _view = new View2D();
        AddChild(_view);

        // Subscribe to the adopted signaling socket's lifecycle so a mid-game
        // session end / kick / drop returns to the menu instead of stranding us.
        _signaling = ctx?.Signaling;
        if (_signaling != null)
        {
            _signaling.SessionEnded += OnSessionEnded;
            _signaling.Kicked += OnKicked;
            _signaling.Closed += OnClosed;
        }

        // Net glue: handoff send/receive over the transport + server-relayed
        // score channel. Only meaningful with both a transport and a signaling
        // socket; without them this is the defensive no-peer fallback.
        if (ctx?.Transport != null && _signaling != null)
            _controller = new NetGameController(_state, ctx.Transport, _signaling, BallRadius);

        // Only the host serves the first ball; everyone else starts empty and
        // receives a ball via handoff (Slice D) or when they're scored on.
        if (ctx?.LocalPlayerIsHost == true)
            SpawnServeBall();

        _view.Render(_state);
    }

    public override void _ExitTree()
    {
        // Detach so the surviving socket doesn't call into a freed scene if it
        // emits another event after we leave.
        _controller?.Dispose();
        _controller = null;
        if (_signaling != null)
        {
            _signaling.SessionEnded -= OnSessionEnded;
            _signaling.Kicked -= OnKicked;
            _signaling.Closed -= OnClosed;
            _signaling = null;
        }
    }

    private void OnSessionEnded(string reason) => LeaveToMenu($"Session ended ({reason}).");

    private void OnKicked(string reason) => LeaveToMenu($"Removed from session ({reason}).");

    private void OnClosed(int code, string reason) => LeaveToMenu(
        code == 1000 ? "Disconnected from session." : $"Connection closed ({code}).");

    private void LeaveToMenu(string message)
    {
        if (_leaving)
            return;
        _leaving = true;
        if (!string.IsNullOrEmpty(message))
            GD.Print($"[game] {message}");

        var ctx = SessionContext.Instance;
        ctx?.TeardownNet();
        ctx?.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }

    /// <summary>Bottom is the goal; present peers (sorted for determinism) take
    /// Top/Right/Left; any unfilled slot is a wall.</summary>
    private void BuildEdges(SessionContext? ctx)
    {
        _state.Edges[Edge.Bottom] = EdgeTarget.Goal;

        var peers = new List<string>();
        if (ctx != null)
            foreach (var pid in ctx.PlayerIds)
                if (pid != _state.LocalPlayerId)
                    peers.Add(pid);
        peers.Sort(string.CompareOrdinal);

        for (int i = 0; i < PortalSlots.Length; i++)
            _state.Edges[PortalSlots[i]] =
                i < peers.Count ? EdgeTarget.Portal(peers[i]) : EdgeTarget.Wall;
    }

    public override void _PhysicsProcess(double delta)
    {
        var dt = (float)delta;

        // Paddle: Left/Right arrows. GetAxis returns +1 toward paddle_right.
        float dir = Input.GetAxis("paddle_left", "paddle_right");
        var paddle = _state.Paddle;
        float half = paddle.Width * 0.5f;
        paddle.CenterX = Mathf.Clamp(
            paddle.CenterX + dir * PaddleSpeed * dt, half, _state.ArenaWidth - half);

        if (_awaitingServe && _serveBall != null)
        {
            // Rest the un-served ball on the paddle until the player serves it.
            _serveBall.Pos = new Vector2(paddle.CenterX, paddle.Y - _serveBall.Radius);
            if (Input.IsActionJustPressed("serve"))
            {
                _serveBall.Vel = new Vector2(0, -ServeSpeed);
                _awaitingServe = false;
                _serveBall = null;
            }
        }
        else
        {
            GameSimulation.Step(_state, delta, _step);

            // Hand off any balls that left this screen to the peer across the
            // crossed edge (directed Send, not a broadcast).
            foreach (var handoff in _step.Handoffs)
                _controller?.SendHandoff(handoff);

            foreach (var score in _step.Scores)
                OnScore(score);
        }

        _view.Render(_state);
    }

    private void OnScore(ScoreEvent e)
    {
        // Report to the server (server-relayed scoring) — the controller drops
        // empty scorers (self-goal / untouched). The scored-on player (this
        // client) always serves the next ball regardless of whether a point was
        // awarded.
        _controller?.ReportScore(e);
        SpawnServeBall();
    }

    private void SpawnServeBall()
    {
        _state.Balls.Clear(); // single ball for now (multi-ball is future work)
        _serveBall = new Ball
        {
            Id = _state.NextBallId(),
            Radius = BallRadius,
            Pos = new Vector2(_state.Paddle.CenterX, _state.Paddle.Y - BallRadius),
        };
        _state.Balls.Add(_serveBall);
        _awaitingServe = true;
    }
}
