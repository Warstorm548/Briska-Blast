using System;
using System.Collections.Generic;
using System.Text;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Game.View;

/// <summary>
/// 2D presentation of a <see cref="GameState"/>. Holds child sprites for the
/// background, paddle and each ball (keyed by ball id so balls can appear and
/// vanish across handoffs), and outlines the four edges in <see cref="_Draw"/>
/// colour-coded by kind. Observes state only — never mutates it.
/// </summary>
public partial class View2D : Node2D, IGameView
{
    private static readonly Color WallColor = new(0.6f, 0.6f, 0.6f);
    private static readonly Color PortalColor = new(0.3f, 0.6f, 1f);
    private static readonly Color GoalColor = new(1f, 0.35f, 0.35f);

    private Sprite2D _background = null!;
    private Sprite2D _paddle = null!;
    private Label _scoreboard = null!;
    private readonly Dictionary<int, Sprite2D> _ballSprites = new();
    private readonly Dictionary<int, Sprite2D> _splitterSprites = new();

    // Reused across frames so Render allocates nothing in the hot loop.
    private readonly HashSet<int> _seen = new();
    private readonly List<int> _gone = new();
    private readonly HashSet<int> _seenSplitters = new();
    private readonly List<int> _goneSplitters = new();
    private readonly List<string> _scoreOrder = new();
    private readonly StringBuilder _scoreText = new();

    private GameState? _state;
    private bool _barriersBuilt;

    /// <summary>Resolves a player_id to the display name shown on the scoreboard.
    /// Injected by <c>GameScene</c> from <c>SessionContext.DisplayNameFor</c>. When
    /// null (e.g. a view rendered in isolation) the scoreboard falls back to the
    /// raw id, so the numeric id is never shown as long as a resolver is wired.</summary>
    public Func<string, string>? NameResolver { get; set; }

    public override void _Ready()
    {
        var sprites = SpriteRegistry.Instance;

        // Textures come from the central registry (the single source of truth).
        _background = new Sprite2D
        {
            Texture = sprites.GetTexture(AssetId.Background),
            Centered = false,
            ZIndex = -10,
        };
        AddChild(_background);

        _paddle = new Sprite2D { Texture = sprites.GetTexture(AssetId.Paddle) };
        AddChild(_paddle);

        // Ball textures are resolved per ball in Render (by kind), so the registry
        // loads the BallBT art lazily — the build runs before it's imported.

        // Minimal scoreboard, top-left. Sized large so it's visible on the
        // 2560-wide design viewport without further theming.
        _scoreboard = new Label { Position = new Vector2(24, 16), ZIndex = 10 };
        _scoreboard.AddThemeFontSizeOverride("font_size", 48);
        AddChild(_scoreboard);
    }

    /// <summary>Create the four static corner-barrier sprites once. They never move, so
    /// this runs a single time. Each sprite pivots on its bottom-left pixel
    /// (<c>Offset = (0, -texH)</c>) so the per-corner 90° rotation hinges on that pixel
    /// pinned to the screen corner — the same <see cref="CornerBarrier"/> layout the
    /// simulation derives its collision rects from, so art and collider stay aligned.
    /// Drawn just above the background and below the balls.</summary>
    private void BuildBarriersOnce(GameState state)
    {
        if (_barriersBuilt)
            return;
        var tex = SpriteRegistry.Instance.GetTexture(AssetId.CornerBarrier);
        float texH = tex.GetSize().Y;
        if (texH <= 0f)
            return; // texture not ready yet — retry on the next frame
        float scale = CornerBarrier.ScaleFor(state.ArenaHeight);
        foreach (var (corner, rotation) in CornerBarrier.Corners)
        {
            AddChild(new Sprite2D
            {
                Texture = tex,
                Centered = false,
                Offset = new Vector2(0, -texH),
                Position = CornerBarrier.Pivot(corner, state.ArenaWidth, state.ArenaHeight),
                Rotation = rotation,
                Scale = new Vector2(scale, scale),
                ZIndex = -1,
            });
        }
        _barriersBuilt = true;
    }

    public void Render(GameState state)
    {
        _state = state;
        BuildBarriersOnce(state);

        // Background stretched to fill the arena.
        var bgSize = _background.Texture.GetSize();
        if (bgSize.X > 0 && bgSize.Y > 0)
            _background.Scale = new Vector2(state.ArenaWidth / bgSize.X, state.ArenaHeight / bgSize.Y);

        // Paddle: centre on its bottom-anchored position, scaled to its dims.
        var p = state.Paddle;
        var paddleSize = _paddle.Texture.GetSize();
        _paddle.Position = new Vector2(p.CenterX, p.Y + p.Height * 0.5f);
        if (paddleSize.X > 0 && paddleSize.Y > 0)
            _paddle.Scale = new Vector2(p.Width / paddleSize.X, p.Height / paddleSize.Y);

        // Balls: create/move sprites by id, free those whose ball is gone. The
        // texture is chosen by ball kind (master vs BallBT split) via the registry.
        _seen.Clear();
        foreach (var ball in state.Balls)
        {
            _seen.Add(ball.Id);
            if (!_ballSprites.TryGetValue(ball.Id, out var sprite))
            {
                var tex = SpriteRegistry.Instance.GetTexture(
                    ball.Kind == BallKind.Master ? AssetId.MasterBall : AssetId.BallBT);
                sprite = new Sprite2D { Texture = tex };
                AddChild(sprite);
                _ballSprites[ball.Id] = sprite;
            }
            sprite.Position = ball.Pos;
            var ballSize = sprite.Texture.GetSize();
            if (ballSize.X > 0 && ballSize.Y > 0)
                sprite.Scale = new Vector2(ball.Radius * 2f / ballSize.X, ball.Radius * 2f / ballSize.Y);
        }

        _gone.Clear();
        foreach (var id in _ballSprites.Keys)
            if (!_seen.Contains(id))
                _gone.Add(id);
        foreach (var id in _gone)
        {
            _ballSprites[id].QueueFree();
            _ballSprites.Remove(id);
        }

        // Splitters (system spawns): same create/move/free-by-id pattern as balls.
        _seenSplitters.Clear();
        foreach (var sp in state.Splitters)
        {
            _seenSplitters.Add(sp.Id);
            if (!_splitterSprites.TryGetValue(sp.Id, out var sprite))
            {
                sprite = new Sprite2D { Texture = SpriteRegistry.Instance.GetTexture(AssetId.BallSpliter) };
                AddChild(sprite);
                _splitterSprites[sp.Id] = sprite;
            }
            sprite.Position = sp.Pos;
            var size = sprite.Texture.GetSize();
            if (size.X > 0 && size.Y > 0)
                sprite.Scale = new Vector2(sp.Radius * 2f / size.X, sp.Radius * 2f / size.Y);
        }

        _goneSplitters.Clear();
        foreach (var id in _splitterSprites.Keys)
            if (!_seenSplitters.Contains(id))
                _goneSplitters.Add(id);
        foreach (var id in _goneSplitters)
        {
            _splitterSprites[id].QueueFree();
            _splitterSprites.Remove(id);
        }

        // Scoreboard: "<name>: N  <name>: N  ..." sorted by player_id (stable and
        // unique, so duplicate display names never reorder columns); each id is
        // rendered as its username via NameResolver, falling back to the raw id
        // only when no resolver/name is available. Reusing the StringBuilder +
        // sort buffer keeps Render allocation-free apart from the resolver call.
        _scoreOrder.Clear();
        foreach (var pid in state.Scores.Keys)
            _scoreOrder.Add(pid);
        _scoreOrder.Sort(string.CompareOrdinal);

        _scoreText.Clear();
        foreach (var pid in _scoreOrder)
        {
            if (_scoreText.Length > 0)
                _scoreText.Append("   ");
            var name = NameResolver?.Invoke(pid) ?? pid;
            _scoreText.Append(name).Append(": ").Append(state.Scores[pid]);
        }
        _scoreboard.Text = _scoreText.ToString();

        QueueRedraw(); // refresh the edge outlines
    }

    public override void _Draw()
    {
        if (_state is not { } s)
            return;
        float w = s.ArenaWidth, h = s.ArenaHeight;
        DrawEdge(Edge.Top, new Vector2(0, 0), new Vector2(w, 0));
        DrawEdge(Edge.Right, new Vector2(w, 0), new Vector2(w, h));
        DrawEdge(Edge.Bottom, new Vector2(0, h), new Vector2(w, h));
        DrawEdge(Edge.Left, new Vector2(0, 0), new Vector2(0, h));
    }

    private void DrawEdge(Edge edge, Vector2 a, Vector2 b)
    {
        var kind = _state!.Edges.TryGetValue(edge, out var t) ? t.Kind : EdgeKind.Wall;
        var color = kind switch
        {
            EdgeKind.Goal => GoalColor,
            EdgeKind.Portal => PortalColor,
            _ => WallColor,
        };
        DrawLine(a, b, color, 6f);
    }
}
