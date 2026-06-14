using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game.View;
using BriskaBlast.Net;
using BriskaBlast.UI.Menus;

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

    // Esc-bound pause menu (the design mockup). Null while closed; while open the
    // match stays live underneath but local paddle/serve input is suspended.
    private PauseMenu? _pauseMenu;
    private bool _paused;

    // Reconnect grace overlay. A client shows at most one message at a time:
    // its own socket dropped (self), the host's (host), or a peer's (peer).
    private CanvasLayer _overlayLayer = null!;
    private Label _overlay = null!;
    private bool _selfReconnecting;
    private bool _hostReconnecting;
    private bool _peerReconnecting;
    private ulong _peerReconnectHideMsec;

    // Always-visible session code so players can reshare it with a friend who
    // dropped and needs to re-enter it to rejoin the match.
    private Label _codeLabel = null!;

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
        // Label the scoreboard by username (server-provided, learned via the
        // signaling roster) instead of the internal player_id. Null-safe: a view
        // without a resolver falls back to the raw id.
        _view.NameResolver = ctx != null ? ctx.DisplayNameFor : null;
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
            _signaling.PeerReconnecting += OnPeerReconnecting;
            _signaling.Reconnecting += OnSelfReconnecting;
            _signaling.Reconnected += OnSelfReconnected;
        }

        // Net glue: handoff send/receive over the transport + server-relayed
        // score channel. Only meaningful with both a transport and a signaling
        // socket; without them this is the defensive no-peer fallback.
        if (ctx?.Transport != null && _signaling != null)
            _controller = new NetGameController(_state, ctx.Transport, _signaling, _ballRadius);

        // On a fresh start the host serves the first ball; everyone else starts
        // empty and receives a ball via handoff or when they're scored on. On a
        // REjoin the ball is already in play elsewhere, so a returning host must
        // NOT spawn a second one — consume the flag either way.
        bool isRejoin = ctx?.RejoinInProgress == true;
        if (ctx != null)
            ctx.RejoinInProgress = false;
        if (ctx?.LocalPlayerIsHost == true && !isRejoin)
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
            _signaling.PeerReconnecting -= OnPeerReconnecting;
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

    // ---- Esc pause menu ----

    private void TogglePauseMenu()
    {
        if (_pauseMenu != null)
            ClosePauseMenu();
        else
            OpenPauseMenu();
    }

    private void OpenPauseMenu()
    {
        // Mid-leave the scene is on its way out — don't pop a menu over it.
        if (_leaving || _pauseMenu != null)
            return;
        _pauseMenu = GD.Load<PackedScene>("res://src/ui/menus/PauseMenu.tscn")
            .Instantiate<PauseMenu>();
        _pauseMenu.ReturnRequested += ClosePauseMenu;
        _pauseMenu.ExitToMenuRequested += OnExitToMenu;
        _pauseMenu.QuitRequested += OnQuitGame;
        AddChild(_pauseMenu);
        _paused = true;
    }

    // "Return to Session": just dismiss the overlay and resume play.
    private void ClosePauseMenu()
    {
        if (_pauseMenu == null)
            return;
        _pauseMenu.ReturnRequested -= ClosePauseMenu;
        _pauseMenu.ExitToMenuRequested -= OnExitToMenu;
        _pauseMenu.QuitRequested -= OnQuitGame;
        _pauseMenu.QueueFree();
        _pauseMenu = null;
        _paused = false;
    }

    // "Exit to main menu": leave WITHOUT an explicit `leave` frame, so the server
    // treats us as a transient drop — holding our slot for the 2-min reconnect
    // window and, if we were host, running the 30s promotion grace, exactly as if
    // we'd dropped. LeaveToMenu's TeardownNet does the clean WS close that arms
    // that path (contrast a deliberate Leave, which would promote immediately).
    private void OnExitToMenu() => LeaveToMenu("");

    // "Quit Game": same transient-drop teardown (peers keep our slot and run the
    // grace timers), then fully close the app. Even if the clean close doesn't
    // flush before exit, the dropped socket is still a non-`leave` disconnect, so
    // the server arms the same grace.
    private void OnQuitGame()
    {
        if (_leaving)
            return;
        _leaving = true;
        var ctx = SessionContext.Instance;
        ctx?.TeardownNet();
        ctx?.ClearSession();
        GetTree().Quit();
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

    private void OnPeerReconnecting(string playerId, int graceSecs)
    {
        // A non-host peer dropped mid-game. Show a brief hint; their slot is held
        // longer (for a manual rejoin), but the overlay only flags the window —
        // auto-hide after graceSecs (checked in _PhysicsProcess), or sooner if
        // the mesh heals. The ball keeps flowing over the rest of the mesh.
        _peerReconnecting = true;
        _peerReconnectHideMsec = Time.GetTicksMsec() + (ulong)Mathf.Max(graceSecs, 0) * 1000UL;
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

        // Session code, top-left, so a player can read it back to a dropped
        // friend who needs to re-enter it on the Join screen to rejoin.
        var code = SessionContext.Instance?.SessionCode ?? "";
        _codeLabel = new Label { Text = $"Code: {code}" };
        _codeLabel.SetAnchorsPreset(Control.LayoutPreset.TopLeft);
        _codeLabel.Position = new Vector2(16, 12);
        _codeLabel.AddThemeFontSizeOverride("font_size", 24);
        _overlayLayer.AddChild(_codeLabel);
    }

    private void UpdateOverlay()
    {
        string msg =
            _selfReconnecting ? "Reconnecting…" :
            _hostReconnecting ? "Host reconnecting…" :
            _peerReconnecting ? "A player is reconnecting…" :
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

        // Auto-hide the "a player is reconnecting…" hint once its window elapses.
        if (_peerReconnecting && Time.GetTicksMsec() >= _peerReconnectHideMsec)
        {
            _peerReconnecting = false;
            UpdateOverlay();
        }

        // Escape toggles the in-match pause menu (open ⇄ Return to Session).
        if (Input.IsActionJustPressed("ui_cancel"))
            TogglePauseMenu();

        // Paddle: Left/Right arrows. GetAxis returns +1 toward paddle_right.
        // Suspended while the pause menu is open — the match stays live underneath
        // (a P2P round can't truly pause for everyone) but we stop driving input.
        var paddle = _state.Paddle;
        if (!_paused)
        {
            float dir = Input.GetAxis("paddle_left", "paddle_right");
            float half = paddle.Width * 0.5f;
            paddle.CenterX = Mathf.Clamp(
                paddle.CenterX + dir * _paddleSpeed * dt, half, _state.ArenaWidth - half);
        }

        if (_awaitingServe && _serveBall != null)
        {
            // Rest the un-served ball on the paddle until the player serves it.
            _serveBall.Pos = new Vector2(paddle.CenterX, paddle.Y - _serveBall.Radius);
            if (!_paused && Input.IsActionJustPressed("serve"))
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
