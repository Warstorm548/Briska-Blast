using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game.View;
using BriskaBlast.Net;
using BriskaBlast.UI;
using BriskaBlast.UI.Chat;
using BriskaBlast.UI.Menus;

namespace BriskaBlast.Game;

/// <summary>
/// Runs one player's screen of an Extended-mode round. Owns the authoritative
/// <see cref="GameState"/> for this screen, steps <see cref="GameSimulation"/>
/// each physics frame, and drives a <see cref="View2D"/>. Entered by
/// <see cref="MatchFlow"/> once the WebRTC mesh is up (never standalone); the
/// live signaling + transport are read from the orchestrator, and every exit
/// (menu, quit, host-again) routes back through its one teardown. Lifecycle
/// events (session end, kick, terminal close, game over) are MatchFlow's —
/// this scene subscribes only to the pure-UI signaling events (reconnect
/// overlays) and to MatchFlow's typed <c>MatchEnded</c>.
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

    // Esc-bound pause menu (the design mockup). Null while closed; while open the
    // match stays live underneath but local paddle/serve input is suspended.
    private PauseMenu? _pauseMenu;
    private bool _paused;

    // End-game overlay, shown on the server's GameOver. Once `_gameOver` is set the
    // simulation is frozen and the EndGameMenu owns navigation; the following
    // SessionEnded teardown is ignored so it can't yank the player off the board.
    private EndGameMenu? _endGameMenu;
    private bool _gameOver;

    // Pause-on-rejoin freeze (server match_paused/match_resumed): while set,
    // _PhysicsProcess steps nothing — every screen freezes together so balls
    // aren't sent at the rejoiner's still-walled edge. Mirrors the _gameOver
    // latch; resolved by the resume countdown reaching zero. The PreparingPanel
    // is reused as the overlay ("Waiting for {name}…", no Cancel — mid-match
    // there is nothing local to cancel).
    private PreparingPanel? _pausePanel;
    private bool _flowPaused;
    private bool _resumeCountdownActive;
    private ulong _resumeAtMsec;

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

    // The action bar below the play field. Its CanvasLayer sits under the reconnect
    // overlay (100) and the pause / end-game menus (200) so both still cover it.
    private const int HotbarLayer = 50;
    private HotbarView _hotbar = null!;

    // In-match chat, sharing the bottom strip with the action bar. Above the bar
    // so the panel's expanded state is not clipped by it, still below the
    // reconnect overlay (100) and the menus (200).
    private const int ChatLayer = 60;
    private InGameChat _chat = null!;

    // The ranked leaderboard, top-left. Above chat so its glow is not clipped by
    // the strip, still below the reconnect overlay (100) and the menus (200).
    private const int LeaderboardLayer = 70;
    private LeaderboardView _leaderboard = null!;

    // Chat holds the keyboard. Every input read below is polled from the device,
    // and polling ignores GUI focus entirely — without this latch, typing "1"-"5"
    // fires hotbar slots, Space serves the ball, and the arrow keys that move the
    // caret also slide the paddle. The match keeps running underneath: chatting
    // mid-rally is the player's own risk, by design.
    private bool _chatFocused;

    /// <summary>Input action per hotbar slot, indexed by slot. Held as a table because
    /// _PhysicsProcess polls all of them every frame and building the names inline would
    /// allocate a string per slot per frame. This is also the authority on which slots
    /// are reachable: growing <see cref="Hotbar.SlotCount"/> without adding matching
    /// actions here (and in project.godot) just leaves the extra slots keyless.</summary>
    private static readonly string[] HotbarActions =
    {
        "hotbar_slot_1", "hotbar_slot_2", "hotbar_slot_3", "hotbar_slot_4", "hotbar_slot_5",
    };

    /// <summary>Stand the match up: resolve the arena from this client's viewport,
    /// build the view, action bar, chat and overlays, subscribe to the flow and the
    /// socket, and take the cursor away for the duration of play.</summary>
    public override void _Ready()
    {
        var ctx = SessionContext.Instance;

        // The action bar owns a strip along the bottom of the screen and the play field
        // is everything above it, so "arena" here is deliberately SMALLER than the
        // viewport. Resolving it once means every size below — the paddle line, the
        // corner-barrier colliders, the ball radius, ArenaWidth/Height — follows the
        // shrink for free, and the colliders keep being built from the same numbers the
        // view draws their sprites from (see CornerBarrier's single-source-of-truth note).
        var viewport = GetViewportRect().Size;
        var arena = new Vector2(viewport.X, viewport.Y - HotbarView.HeightHFrac * viewport.Y);

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

        // Pure play-field renderer now: the scoreboard it used to carry became
        // LeaderboardView, which resolves its own usernames.
        _view = new View2D();
        AddChild(_view);

        // The action bar fills the strip the arena just gave up. Its own CanvasLayer
        // sits below the reconnect overlay (100) and the pause / end-game menus (200),
        // so those still cover it.
        _hotbar = new HotbarView { Layer = HotbarLayer };
        AddChild(_hotbar);
        _hotbar.SyncFrom(_state.Hotbar);

        // Chat shares the strip with the action bar. It binds the transcript
        // MatchFlow carried in at the Preparing handoff, so the lobby
        // conversation is already on screen before the first serve — nothing is
        // re-fetched from the server.
        _chat = new InGameChat { Layer = ChatLayer };
        AddChild(_chat);
        _chat.Bind(MatchFlow.Instance.Chat);
        _chat.FocusChanged += OnChatFocusChanged;

        // The leaderboard owns the top-left corner; the session code moved right to
        // make room for it (see BuildOverlay).
        _leaderboard = new LeaderboardView { Layer = LeaderboardLayer };
        AddChild(_leaderboard);

        BuildOverlay();
        UpdateCursor();

        // The live net belongs to MatchFlow (this scene is only entered by it,
        // post-mesh). Lifecycle events — session end, kick, terminal close —
        // are the orchestrator's; here we take the game-over relay plus the
        // pure-UI reconnect overlays. HostChanged is overlay-clear only: the
        // roster mutation is MatchFlow's.
        var flow = MatchFlow.Instance;
        flow.MatchEnded += OnGameOver;
        flow.MatchPausedFor += OnMatchPaused;
        flow.MatchResumedIn += OnMatchResumed;
        _signaling = flow.Signaling;
        if (_signaling != null)
        {
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
        if (flow.Transport != null && _signaling != null)
            _controller = new NetGameController(_state, flow.Transport, _signaling, _ballRadius);

        // On a fresh start the host serves the first ball; everyone else starts
        // empty and receives a ball via handoff or when they're scored on. On a
        // REjoin the ball is already in play elsewhere, so a returning host must
        // NOT spawn a second one.
        if (ctx?.LocalPlayerIsHost == true && !flow.IsRejoin)
            SpawnServeBall();

        _view.Render(_state);
    }

    /// <summary>Leave the match: hand the cursor back, then detach from the
    /// controller, the flow and the socket so nothing calls into a freed scene.
    /// Every exit passes through here, which is what makes it the right place for
    /// state that outlives the scene.</summary>
    public override void _ExitTree()
    {
        // The cursor is hidden for the match but Input.MouseMode is global, so it
        // has to be handed back or the menu we are leaving for arrives with no
        // pointer. Unconditional, and here rather than in the exit handlers:
        // every way out of a match — the pause menu, the end screen, a kick, a
        // FailFlow — leaves by way of the tree, and this is the one place all of
        // them pass through.
        Input.MouseMode = Input.MouseModeEnum.Visible;

        // Detach so the surviving socket doesn't call into a freed scene if it
        // emits another event after we leave.
        _controller?.Dispose();
        _controller = null;
        MatchFlow.Instance.MatchEnded -= OnGameOver;
        MatchFlow.Instance.MatchPausedFor -= OnMatchPaused;
        MatchFlow.Instance.MatchResumedIn -= OnMatchResumed;
        if (_chat != null)
            _chat.FocusChanged -= OnChatFocusChanged;
        if (_signaling != null)
        {
            _signaling.HostChanged -= OnHostChangedInGame;
            _signaling.HostReconnecting -= OnHostReconnecting;
            _signaling.HostReconnected -= OnHostReconnected;
            _signaling.PeerReconnecting -= OnPeerReconnecting;
            _signaling.Reconnecting -= OnSelfReconnecting;
            _signaling.Reconnected -= OnSelfReconnected;
            _signaling = null;
        }
    }

    // ---- end-of-match (MatchFlow's GameOver relay) ----

    /// <summary>A player met the win condition. Freeze the sim, clear the pause
    /// overlays and chat's hold on the keyboard, then put the end screen up and
    /// give the cursor back for its buttons. Idempotent — a second relay of the
    /// same result finds <c>_gameOver</c> already set.</summary>
    private void OnGameOver(string winnerPlayerId, Dictionary<string, int> scores)
    {
        if (_gameOver)
            return;
        _gameOver = true;

        // Adopt the server's final tally so the frozen board and the leaderboard
        // are exact even if the preceding ScoreUpdate was missed.
        _state.ApplyScores(scores);
        _view.Render(_state); // one last paint, then the sim freezes (_PhysicsProcess early-returns)
        _leaderboard.SyncFrom(_state);

        // The match is over: drop the pause overlays if they were open (the Esc
        // menu and a pause-on-rejoin hold alike — the end screen supersedes
        // both), then show it on top of the frozen game.
        if (_pauseMenu != null)
            ClosePauseMenu();
        // Same for chat: the end screen takes the keyboard for its own buttons,
        // so the input must not still be holding it.
        _chat.ReleaseInput();
        _flowPaused = false;
        _resumeCountdownActive = false;
        RemovePausePanel();

        _endGameMenu = GD.Load<PackedScene>("res://src/ui/menus/EndGameMenu.tscn")
            .Instantiate<EndGameMenu>();
        _endGameMenu.MainMenuRequested += OnEndGameMainMenu;
        _endGameMenu.HostRequested += OnEndGameHost;
        AddChild(_endGameMenu);
        _endGameMenu.Populate(winnerPlayerId, scores);
        // Play is over and the end screen is a menu: the pointer comes back. Last,
        // because the rule reads _endGameMenu and the ClosePauseMenu above already
        // ran it once while this was still null.
        UpdateCursor();
    }

    // "Return to Main Menu": the one MatchFlow teardown (idempotent — a second
    // press or a racing lifecycle event finds the flow already Idle).
    private void OnEndGameMainMenu() => MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);

    // "Host Game": tear the finished session down, then go set up a new one.
    private void OnEndGameHost() =>
        MatchFlow.Instance.EndMatchTo("res://src/ui/menus/HostSetupMenu.tscn");

    // ---- Esc pause menu ----

    // Chat took or gave back the keyboard. The latch is what actually suspends
    // play — see the _chatFocused field for why focus alone cannot.
    private void OnChatFocusChanged(bool focused) => _chatFocused = focused;

    /// <summary>
    /// The whole cursor policy, in one rule: nothing in play needs a pointer, so
    /// there is none, and it comes back only while a menu with clickable controls
    /// is up. The pause menu's Copy-code button is mouse-only, and the end screen
    /// is a menu; those two are the entire list of things that need it today.
    ///
    /// Hidden rather than Captured: Captured warps the pointer to the centre of
    /// the window and is meant for mouselook, which would fight windowed play and
    /// alt-tab. Hidden leaves the OS pointer where it is — which is also why it
    /// alone is not enough: a hidden cursor still delivers clicks, so the chat
    /// panel is made click-through as well (see <c>InGameChat</c>).
    ///
    /// The status overlays are deliberately absent from the rule: the reconnect
    /// overlay and the rejoin pause panel are labels, with nothing to click.
    /// </summary>
    private void UpdateCursor() =>
        Input.MouseMode = _pauseMenu != null || _endGameMenu != null
            ? Input.MouseModeEnum.Visible
            : Input.MouseModeEnum.Hidden;

    private void TogglePauseMenu()
    {
        if (_pauseMenu != null)
            ClosePauseMenu();
        else
            OpenPauseMenu();
    }

    private void OpenPauseMenu()
    {
        // Only while actually playing — mid-leave (flow already Idle) or
        // post-match the scene is on its way out; don't pop a menu over it.
        if (MatchFlow.Instance.State != MatchFlowState.InMatch || _pauseMenu != null)
            return;
        // The menu grabs focus for its own buttons, so hand the keyboard back
        // first rather than leaving chat holding a latch it can no longer clear.
        _chat.ReleaseInput();
        _pauseMenu = GD.Load<PackedScene>("res://src/ui/menus/PauseMenu.tscn")
            .Instantiate<PauseMenu>();
        _pauseMenu.ReturnRequested += ClosePauseMenu;
        _pauseMenu.ExitToMenuRequested += OnExitToMenu;
        _pauseMenu.QuitRequested += OnQuitGame;
        AddChild(_pauseMenu);
        _paused = true;
        UpdateCursor();
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
        UpdateCursor();
    }

    // "Exit to main menu": leave WITHOUT an explicit `leave` frame, so the server
    // treats us as a transient drop — holding our slot for the 2-min reconnect
    // window and, if we were host, running the 30s promotion grace, exactly as if
    // we'd dropped (contrast a deliberate Leave, which would promote immediately).
    private void OnExitToMenu() => MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);

    // "Quit Game": same transient-drop teardown (peers keep our slot and run the
    // grace timers), then fully close the app. Even if the clean close doesn't
    // flush before exit, the dropped socket is still a non-`leave` disconnect, so
    // the server arms the same grace.
    private void OnQuitGame() => MatchFlow.Instance.QuitGame();

    // ---- pause-on-rejoin (MatchFlow's match_paused/match_resumed relays) ----

    private void OnMatchPaused(string displayName)
    {
        if (_gameOver)
            return;
        _flowPaused = true;
        // A second rejoiner while already paused just updates the name; a pause
        // landing mid-countdown cancels the countdown (a new hold arrived).
        _resumeCountdownActive = false;

        if (_pausePanel == null)
        {
            _pausePanel = GD.Load<PackedScene>("res://src/ui/menus/PreparingPanel.tscn")
                .Instantiate<PreparingPanel>();
            _overlayLayer.AddChild(_pausePanel);
            _pausePanel.SetAnchorsPreset(Control.LayoutPreset.Center);
            _pausePanel.ShowCancel(false);
        }
        _pausePanel.SetTitle("Match paused");
        _pausePanel.SetStatus($"Waiting for {displayName} to reconnect…");
    }

    private void OnMatchResumed(int countdownSecs)
    {
        if (_gameOver || !_flowPaused)
            return;
        _resumeCountdownActive = true;
        _resumeAtMsec = Time.GetTicksMsec() + (ulong)Mathf.Max(countdownSecs, 0) * 1000UL;
    }

    private void RemovePausePanel()
    {
        _pausePanel?.QueueFree();
        _pausePanel = null;
    }

    /// <summary>Drive the frozen phase each physics tick: paint the resume
    /// countdown once it's running and unfreeze when it reaches zero. Returns
    /// true while the sim must stay frozen.</summary>
    private bool TickFlowPause()
    {
        if (!_flowPaused)
            return false;
        if (!_resumeCountdownActive)
            return true; // waiting on the rejoiner / the server valve

        ulong now = Time.GetTicksMsec();
        if (now < _resumeAtMsec)
        {
            int remaining = (int)((_resumeAtMsec - now + 999) / 1000);
            _pausePanel?.SetTitle("Match resuming");
            _pausePanel?.SetStatus($"Resuming in {remaining}…");
            return true;
        }

        _flowPaused = false;
        _resumeCountdownActive = false;
        RemovePausePanel();
        return false;
    }

    // ---- host-loss grace UI (Stage 4) ----

    private void OnHostChangedInGame(string playerId)
    {
        // Promotion landed (or a voluntary transfer): clear the "host
        // reconnecting…" overlay. The roster/host mutation itself is
        // MatchFlow's — this handler is pure UI.
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

        // Session code, top-RIGHT, so a player can read it back to a dropped friend
        // who needs to re-enter it on the Join screen to rejoin. It sat top-left
        // until 0.34.0, overlapping the scoreboard that lived there; the
        // leaderboard now owns that corner, and the pause menu carries the code
        // with a Copy button anyway, so this is the convenience copy.
        var code = SessionContext.Instance?.SessionCode ?? "";
        _codeLabel = new Label { Text = $"Code: {code}" };
        // Spans the top and right-ALIGNS its text rather than being a right-anchored
        // box: a Label's width comes from its own text, so anchoring the box to the
        // right edge and nudging it would run a longer code off screen.
        _codeLabel.SetAnchorsPreset(Control.LayoutPreset.TopWide);
        _codeLabel.HorizontalAlignment = HorizontalAlignment.Right;
        _codeLabel.OffsetLeft = 0;
        _codeLabel.OffsetRight = -16;
        _codeLabel.OffsetTop = 12;
        _codeLabel.OffsetBottom = 48;
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

        // Paused for a rejoiner: every screen freezes together (input, spawns,
        // sim, handoffs) until the resume countdown runs out.
        if (TickFlowPause())
            return;

        var dt = (float)delta;

        // Auto-hide the "a player is reconnecting…" hint once its window elapses.
        if (_peerReconnecting && Time.GetTicksMsec() >= _peerReconnectHideMsec)
        {
            _peerReconnecting = false;
            UpdateOverlay();
        }

        // Escape toggles the in-match pause menu (open ⇄ Return to Session) —
        // unless chat holds the keyboard, where it is the way out of the input
        // instead. Escape IS consumed by an editing LineEdit, but only to leave
        // edit mode, and that preserves focus (Godot 4.4+ keeps the two apart) —
        // so without this branch chat would sit there holding the latch with no
        // caret, and the paddle would never come back. Polling sidesteps the
        // consumption either way: this reads the raw action, not the event.
        if (Input.IsActionJustPressed("ui_cancel"))
        {
            if (_chatFocused)
                _chat.ReleaseInput();
            else
                TogglePauseMenu();
        }

        // Hotbar: number keys 1-5 fire their own slot. Suspended while the pause menu
        // is open or chat holds the keyboard, like the paddle and the serve; the
        // _gameOver / flow-pause returns above already cover the end screen and a
        // rejoin freeze. Deliberately live during the pre-serve wait — using an item
        // before serving is harmless.
        if (!_paused && !_chatFocused)
        {
            // Bounded by the action table, not the slot count: a slot with no key bound
            // is simply unreachable rather than an index past the end of the table.
            for (int i = 0; i < HotbarActions.Length && i < Hotbar.SlotCount; i++)
                if (Input.IsActionJustPressed(HotbarActions[i]))
                    OnHotbarSlotActivated(i);
        }

        // System spawns (BallSpliter) appear on their own cadence, independent of
        // the serve / paddle — the sim resolves any ball that touches one.
        TickSplitters(delta);

        // Paddle: Left/Right arrows. GetAxis returns +1 toward paddle_right.
        // Suspended while the pause menu is open or chat holds the keyboard — the
        // match stays live underneath (a P2P round can't truly pause for everyone)
        // but we stop driving input. In chat's case that is the whole point: the
        // arrows are moving the caret, not the paddle.
        var paddle = _state.Paddle;
        if (!_paused && !_chatFocused)
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
            if (!_paused && !_chatFocused && Input.IsActionJustPressed("serve"))
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
        // Scores restate every frame; the board settles its ORDER on its own slower
        // beat, which is the point of the split.
        _leaderboard.SyncFrom(_state);
    }

    /// <summary>A hotbar slot's key was pressed. Acknowledges the press on screen, then
    /// activates whatever the slot holds — which is nothing today, since no item system
    /// exists yet to fill one. The empty-slot return below is where item activation
    /// hangs when it arrives.</summary>
    private void OnHotbarSlotActivated(int index)
    {
        // Flash regardless of contents: the player pressed a key and deserves to see
        // that it registered, whether or not the slot had anything in it.
        _hotbar.Flash(index);

        var slot = _state.Hotbar.Slots[index];
        if (slot.IsEmpty)
        {
            Log.Debug("game.hotbar", $"slot {index + 1} pressed (empty)");
            return;
        }

        Log.Debug("game.hotbar", $"slot {index + 1} activated (icon={slot.Icon}, count={slot.Count})");
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
