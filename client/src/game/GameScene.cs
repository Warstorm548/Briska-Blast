using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game.View;
using BriskaBlast.Net;
using BriskaBlast.UI;
using BriskaBlast.UI.Chat;

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

    // Pixel values resolved from this client's arena in _Ready (the ones used
    // after construction; one-shot locals cover the rest).
    private float _paddleSpeed;
    private float _serveSpeed;
    private float _ballRadius;

    // Shared spawn randomness. Seeded once in _Ready and drawn from by both
    // per-frame spawners (GameScene.Splitters.cs and GameScene.Loot.cs).
    private readonly RandomNumberGenerator _rng = new();

    private GameState _state = null!;
    private View2D _view = null!;
    private readonly StepResult _step = new();

    private SignalingClient? _signaling;
    private NetGameController? _controller;

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
            _lootSettings = ctx.LootSettings;
            _lootIntervalSecs = _lootSettings.DropIntervalSecs;
        }
        _rng.Randomize();
        _splitterCooldown = _splitterIntervalSecs;
        _lootCooldown = _lootIntervalSecs;

        // The barrier sits in the gap between the paddle and the goal line. Solved
        // here, after the corner triangles exist, because its ends are derived from
        // them rather than hardcoded — see ResolveShieldGeometry.
        ResolveShieldGeometry(arena);

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
        {
            _controller = new NetGameController(_state, flow.Transport, _signaling, _ballRadius);
            // A peer awarding us loot we earned on their screen. Unsubscribed with
            // the controller in _ExitTree via Dispose.
            _controller.ItemAwarded += GrantItem;
        }

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
        if (_controller != null)
            _controller.ItemAwarded -= GrantItem;
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
}
