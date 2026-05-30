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
    // Tuning placeholders, expressed RELATIVE to the arena so each quantity means
    // the same thing on any screen size or aspect ratio (clients run different
    // arena dimensions on non-16:9 displays — see docs/planning/known-bugs.md).
    // Speeds, the goal gap, the paddle height and the ball radius are fractions of
    // arena HEIGHT; the paddle width is a fraction of arena WIDTH. The fractions
    // reproduce the original feel at the 2560×1440 design size and are resolved to
    // pixels in _Ready from THIS client's own arena.
    private const float PaddleSpeedHFrac = 1400f / 1440f; // arena heights / second
    private const float ServeSpeedHFrac = 900f / 1440f;   // arena heights / second
    private const float GoalGapHFrac = 120f / 1440f;
    private const float PaddleWidthWFrac = 240f / 2560f;
    private const float PaddleHeightHFrac = 36f / 1440f;
    private const float BallRadiusHFrac = 24f / 1440f;

    // Pixel values resolved from this client's arena in _Ready (the ones used
    // after construction; one-shot locals cover the rest).
    private float _paddleSpeed;
    private float _serveSpeed;
    private float _ballRadius;

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

    // Host-loss grace overlay (Stage 4). A client sees at most one of these at a
    // time: it either lost its own socket (self) or sees the host's loss (host).
    private CanvasLayer _overlayLayer = null!;
    private Label _overlay = null!;
    private bool _selfReconnecting;
    private bool _hostReconnecting;

    public override void _Ready()
    {
        var ctx = SessionContext.Instance;
        var arena = GetViewportRect().Size;

        // Resolve the relative tuning to this arena's pixels (height-relative,
        // except the paddle width which is width-relative).
        _paddleSpeed = PaddleSpeedHFrac * arena.Y;
        _serveSpeed = ServeSpeedHFrac * arena.Y;
        _ballRadius = BallRadiusHFrac * arena.Y;

        _state = new GameState
        {
            ArenaWidth = arena.X,
            ArenaHeight = arena.Y,
            LocalPlayerId = ctx?.PlayerId ?? "",
        };
        _state.Paddle.Width = PaddleWidthWFrac * arena.X;
        _state.Paddle.Height = PaddleHeightHFrac * arena.Y;
        _state.Paddle.CenterX = arena.X * 0.5f;
        _state.Paddle.Y = arena.Y - GoalGapHFrac * arena.Y;

        BuildEdges(ctx);

        _view = new View2D();
        AddChild(_view);

        BuildOverlay();

        // Subscribe to the adopted signaling socket's lifecycle so a mid-game
        // session end / kick / terminal drop returns to the menu instead of
        // stranding us. HostChanged / *Reconnecting keep our host notion and the
        // overlay honest as the server's host-loss grace plays out (Stage 4).
        _signaling = ctx?.Signaling;
        if (_signaling != null)
        {
            _signaling.SessionEnded += OnSessionEnded;
            _signaling.Kicked += OnKicked;
            _signaling.Closed += OnClosed;
            _signaling.HostChanged += OnHostChangedInGame;
            _signaling.HostReconnecting += OnHostReconnecting;
            _signaling.HostReconnected += OnHostReconnected;
            _signaling.Reconnecting += OnSelfReconnecting;
            _signaling.Reconnected += OnSelfReconnected;
        }

        // Net glue: handoff send/receive over the transport + server-relayed
        // score channel. Only meaningful with both a transport and a signaling
        // socket; without them this is the defensive no-peer fallback.
        if (ctx?.Transport != null && _signaling != null)
            _controller = new NetGameController(_state, ctx.Transport, _signaling, _ballRadius);

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
            _signaling.HostChanged -= OnHostChangedInGame;
            _signaling.HostReconnecting -= OnHostReconnecting;
            _signaling.HostReconnected -= OnHostReconnected;
            _signaling.Reconnecting -= OnSelfReconnecting;
            _signaling.Reconnected -= OnSelfReconnected;
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

    private void LeaveMatch()
    {
        // Deliberate quit: tell the server (Leave) so peers promote immediately
        // instead of waiting out the host-reconnect grace, then return to menu.
        _signaling?.SendLeave();
        LeaveToMenu("");
    }

    // ---- host-loss grace UI (Stage 4) ----

    private void OnHostChangedInGame(string playerId)
    {
        // Promotion landed (or a voluntary transfer): keep our host notion
        // current and clear the "host reconnecting…" overlay.
        var ctx = SessionContext.Instance;
        if (ctx != null)
            ctx.HostPlayerId = playerId;
        _hostReconnecting = false;
        UpdateOverlay();
    }

    private void OnHostReconnecting(string playerId, int graceSecs)
    {
        _hostReconnecting = true;
        UpdateOverlay();
    }

    private void OnHostReconnected(string playerId)
    {
        _hostReconnecting = false;
        UpdateOverlay();
    }

    private void OnSelfReconnecting()
    {
        _selfReconnecting = true;
        UpdateOverlay();
    }

    private void OnSelfReconnected()
    {
        _selfReconnecting = false;
        UpdateOverlay();
    }

    private void BuildOverlay()
    {
        _overlayLayer = new CanvasLayer { Layer = 100 };
        AddChild(_overlayLayer);
        _overlay = new Label
        {
            Visible = false,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        _overlay.SetAnchorsPreset(Control.LayoutPreset.FullRect);
        _overlay.AddThemeFontSizeOverride("font_size", 64);
        _overlayLayer.AddChild(_overlay);
    }

    private void UpdateOverlay()
    {
        string msg =
            _selfReconnecting ? "Reconnecting…" :
            _hostReconnecting ? "Host reconnecting…" :
            "";
        _overlay.Text = msg;
        _overlay.Visible = msg.Length > 0;
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

        // Escape leaves the match deliberately — sends Leave so peers promote
        // immediately rather than waiting out the host-reconnect grace.
        if (Input.IsActionJustPressed("ui_cancel"))
        {
            LeaveMatch();
            return;
        }

        // Paddle: Left/Right arrows. GetAxis returns +1 toward paddle_right.
        float dir = Input.GetAxis("paddle_left", "paddle_right");
        var paddle = _state.Paddle;
        float half = paddle.Width * 0.5f;
        paddle.CenterX = Mathf.Clamp(
            paddle.CenterX + dir * _paddleSpeed * dt, half, _state.ArenaWidth - half);

        if (_awaitingServe && _serveBall != null)
        {
            // Rest the un-served ball on the paddle until the player serves it.
            _serveBall.Pos = new Vector2(paddle.CenterX, paddle.Y - _serveBall.Radius);
            if (Input.IsActionJustPressed("serve"))
            {
                _serveBall.Vel = new Vector2(0, -_serveSpeed);
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
            Radius = _ballRadius,
            Pos = new Vector2(_state.Paddle.CenterX, _state.Paddle.Y - _ballRadius),
        };
        _state.Balls.Add(_serveBall);
        _awaitingServe = true;
    }
}
