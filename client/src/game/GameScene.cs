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

    /// <summary>Default seconds between BallSpliter spawns, used as a fallback until
    /// the host's setting is read (Stage 3). On average a splitter appears this often
    /// on each screen; the cooldown after one is consumed doubles as its respawn.</summary>
    private const double DefaultSplitterIntervalSecs = 15.0;

    // Pixel values resolved from this client's arena in _Ready (the ones used
    // after construction; one-shot locals cover the rest).
    private float _paddleSpeed;
    private float _serveSpeed;
    private float _ballRadius;

    // Random-spawn (BallSpliter) cadence. Each screen spawns its own splitters
    // locally on this cooldown; the resulting BallBT balls hand off like any other
    // ball. Stage 3 overrides the interval + chain-split from the host's settings.
    private double _splitterIntervalSecs = DefaultSplitterIntervalSecs;
    private double _splitterCooldown;
    private readonly RandomNumberGenerator _rng = new();

    // Seat-relative portal layout (bottom is always the local goal). Seats are
    // arranged on a fixed "table" matching Example Imgs/GameMode Extended.png:
    // seat 0 = P1/Host at the bottom (South), 1 = P2 top (North), 2 = P3 left
    // (West), 3 = P4 right (East). On the local player's own upright screen the
    // peer seated OPPOSITE you is on your Top edge, the one on your right hand on
    // Right, the one on your left on Left. This table is edge[localSeat, peerSeat]
    // (the diagonal is unused — you are never your own peer).
    private static readonly Edge[,] SeatEdge =
    {
        //              peer P1(S)   peer P2(N)   peer P3(W)   peer P4(E)
        /* local P1 */ { Edge.Top,   Edge.Top,    Edge.Left,   Edge.Right },
        /* local P2 */ { Edge.Top,   Edge.Top,    Edge.Right,  Edge.Left  },
        /* local P3 */ { Edge.Right, Edge.Left,   Edge.Top,    Edge.Top   },
        /* local P4 */ { Edge.Left,  Edge.Right,  Edge.Top,    Edge.Top   },
    };

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

    // End-game overlay, shown on the server's GameOver. Once `_gameOver` is set the
    // simulation is frozen and the EndGameMenu owns navigation; the following
    // SessionEnded teardown is ignored so it can't yank the player off the board.
    private EndGameMenu? _endGameMenu;
    private bool _gameOver;

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

        // Solid triangle barriers in all four corners (same on every screen). Static local
        // geometry — built once here so the sim can bounce balls off them and the view
        // can place the sprites from the shared CornerBarrier layout.
        CornerBarrier.AppendTriangles(_state.Barriers, arena.X, arena.Y);

        // Arm the random-spawn cadence from the host's settings (broadcast at start
        // and applied by every client), falling back to the defaults if absent.
        if (ctx != null)
        {
            _splitterIntervalSecs = ctx.SplitterIntervalSecs;
            _state.ChainSplitEnabled = ctx.ChainSplit;
        }
        _rng.Randomize();
        _splitterCooldown = _splitterIntervalSecs;

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
            _signaling.GameOver += OnGameOver;
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
            _signaling.GameOver -= OnGameOver;
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

    private void OnSessionEnded(string reason)
    {
        // After a win the server hands cleanup to SessionEnded right behind the
        // GameOver frame. Once the end screen is up it owns navigation — don't let
        // this auto-leave tear it down. Every other reason behaves as before.
        if (_gameOver)
            return;
        LeaveToMenu($"Session ended ({reason}).");
    }

    private void OnKicked(string reason) => LeaveToMenu($"Removed from session ({reason}).");

    // ---- end-of-match (server GameOver) ----

    private void OnGameOver(string winnerPlayerId, Dictionary<string, int> scores)
    {
        if (_gameOver || _leaving)
            return;
        _gameOver = true;

        // Adopt the server's final tally so the frozen board and the leaderboard
        // are exact even if the preceding ScoreUpdate was missed.
        _state.Scores.Clear();
        foreach (var (pid, pts) in scores)
            _state.Scores[pid] = pts;
        _view.Render(_state); // one last paint, then the sim freezes (_PhysicsProcess early-returns)

        // The match is over: drop the pause overlay if it was open, then show the
        // end screen on top of the frozen game.
        if (_pauseMenu != null)
            ClosePauseMenu();

        _endGameMenu = GD.Load<PackedScene>("res://src/ui/menus/EndGameMenu.tscn")
            .Instantiate<EndGameMenu>();
        _endGameMenu.MainMenuRequested += OnEndGameMainMenu;
        _endGameMenu.HostRequested += OnEndGameHost;
        AddChild(_endGameMenu);
        _endGameMenu.Populate(winnerPlayerId, scores);
    }

    // "Return to Main Menu": same clean teardown as any leave.
    private void OnEndGameMainMenu() => LeaveToMenu("");

    // "Host Game": tear the finished session down, then go set up a new one.
    private void OnEndGameHost()
    {
        if (_leaving)
            return;
        _leaving = true;
        var ctx = SessionContext.Instance;
        ctx?.TeardownNet();
        ctx?.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/HostSetupMenu.tscn");
    }

    private void OnClosed(int code, string reason) => LeaveToMenu(
        code == 1000 ? "Disconnected from session." : $"Connection closed ({code}).");

    private void LeaveToMenu(string message)
    {
        if (_leaving)
            return;
        _leaving = true;
        if (!string.IsNullOrEmpty(message))
            Log.Info("game", message);

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

    /// <summary>Bottom is the goal; every other player is placed on the Top/Left/
    /// Right portal edge dictated by their seat relative to ours (see
    /// <see cref="SeatEdge"/> and Example Imgs/GameMode Extended.png). Any edge
    /// with no peer seated across it is a wall. Seats follow the server's frozen,
    /// join-ordered roster (<see cref="SessionContext.SeatOrder"/>): P1 is the
    /// player who created the lobby (the start-time host — first to join), then the
    /// rest in the order they joined. This runs a single time, in <c>_Ready</c>,
    /// and the roster is frozen server-side at Start, so the layout is identical on
    /// every client and FROZEN for the match: a later host promotion never re-seats
    /// anyone or moves a portal, and a process-death rejoiner reproduces the same
    /// seating. Falls back to host-first + id-sort only if the roster is
    /// unavailable (e.g. an older server that doesn't send <c>seat_order</c>).</summary>
    private void BuildEdges(SessionContext? ctx)
    {
        _state.Edges[Edge.Bottom] = EdgeTarget.Goal;
        _state.Edges[Edge.Top] = EdgeTarget.Wall;
        _state.Edges[Edge.Left] = EdgeTarget.Wall;
        _state.Edges[Edge.Right] = EdgeTarget.Wall;

        if (ctx == null)
            return;

        // Server-authoritative, join-ordered, self-inclusive seating roster,
        // captured once at Start (start_signaling) or rejoin (Identified.seat_order)
        // and identical on every client.
        var seatOrder = new List<string>(ctx.SeatOrder);
        if (seatOrder.Count == 0)
        {
            // Legacy fallback (older server without seat_order): host first, then
            // the remaining members sorted by id — still deterministic everywhere.
            if (!string.IsNullOrEmpty(ctx.HostPlayerId))
                seatOrder.Add(ctx.HostPlayerId);
            var others = new List<string>();
            foreach (var pid in ctx.PlayerIds)
                if (pid != ctx.HostPlayerId && !others.Contains(pid))
                    others.Add(pid);
            others.Sort(string.CompareOrdinal);
            seatOrder.AddRange(others);
        }

        int localSeat = seatOrder.IndexOf(_state.LocalPlayerId);
        if (localSeat < 0 || localSeat >= 4)
            return; // not seated (shouldn't happen in a 2–4 player round) — all walls

        // Namespace this screen's ball ids by seat so balls created on different
        // screens never collide once handed across the mesh. A per-process wall-clock
        // offset within the seat's block makes a process-death rejoin (which restarts
        // the local counter at 0) begin in a different sub-range, so its new ids can't
        // collide with same-seat balls it served before dropping that may still be in
        // play elsewhere.
        int idOffset = (int)((long)(Time.GetUnixTimeFromSystem() * 1000.0) % GameState.BallIdRejoinOffsetRange);
        _state.BallIdBase = localSeat * GameState.BallIdSeatStride + idOffset;

        for (int peerSeat = 0; peerSeat < seatOrder.Count && peerSeat < 4; peerSeat++)
        {
            if (peerSeat == localSeat)
                continue;
            _state.Edges[SeatEdge[localSeat, peerSeat]] =
                EdgeTarget.Portal(seatOrder[peerSeat]);
        }
    }

    public override void _PhysicsProcess(double delta)
    {
        // Match over: the simulation is frozen behind the end screen. Step nothing,
        // accept no input — the last paint in OnGameOver stays on screen.
        if (_gameOver)
            return;

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

        // System spawns (BallSpliter) appear on their own cadence, independent of
        // the serve / paddle — the sim resolves any ball that touches one.
        TickSplitters(delta);

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

        // Always advance the simulation so every ball in play keeps moving — with
        // multi-ball, an un-served master resting on the paddle must not freeze the
        // split balls still bouncing around. The held serve ball has zero velocity,
        // so Step leaves it untouched; it's glued to the paddle just below.
        GameSimulation.Step(_state, delta, _step);

        // Hand off any balls that left this screen to the peer across the crossed
        // edge (directed Send, not a broadcast).
        foreach (var handoff in _step.Handoffs)
            _controller?.SendHandoff(handoff);

        foreach (var score in _step.Scores)
            OnScore(score);

        if (_awaitingServe && _serveBall != null)
        {
            // Rest the un-served ball on the paddle until the player serves it.
            _serveBall.Pos = new Vector2(paddle.CenterX, paddle.Y - _serveBall.Radius);
            if (!_paused && Input.IsActionJustPressed("serve"))
            {
                _serveBall.Vel = new Vector2(0, -_serveSpeed);
                // Serving applies force, so it counts as a hit: tag the ball with
                // the server's id (same as a paddle deflection). A later paddle
                // hit by anyone overwrites this, so credit always follows the last
                // player to act on the ball. This lets a clean serve that crosses
                // into a peer's goal untouched score for the server, instead of
                // dying as an "untouched" ball that credited nobody.
                _serveBall.LastHitterId = _state.LocalPlayerId;
                _awaitingServe = false;
                _serveBall = null;
            }
        }

        _view.Render(_state);
    }

    private void OnScore(ScoreEvent e)
    {
        // Report to the server (server-relayed scoring) — the controller drops
        // empty scorers (self-goal / untouched).
        _controller?.ReportScore(e);

        // Only a lost master ball is replaced: the scored-on player serves the next
        // one. A split (BallBT) ball is a bonus — it just vanishes, no re-serve.
        if (e.Kind == BallKind.Master)
            SpawnServeBall();
    }

    private void SpawnServeBall()
    {
        // Serve a fresh master ball. Any split balls in play are left alone — only
        // the lost master is replaced. Exactly one master exists at a time (it's the
        // single ball handed between screens), so there's nothing to clear here.
        _serveBall = new Ball
        {
            Id = _state.NextBallId(),
            Radius = _ballRadius,
            Kind = BallKind.Master,
            Pos = new Vector2(_state.Paddle.CenterX, _state.Paddle.Y - _ballRadius),
        };
        _state.Balls.Add(_serveBall);
        _awaitingServe = true;
    }

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

    /// <summary>True if a circle at <paramref name="pos"/> overlaps any corner barrier.</summary>
    private bool OverlapsBarrier(Vector2 pos, float radius)
    {
        foreach (var tri in _state.Barriers)
            if (CornerBarrier.Overlaps(tri, pos, radius))
                return true;
        return false;
    }
}
