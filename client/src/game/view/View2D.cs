using System.Collections.Generic;
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
    private readonly Dictionary<int, Sprite2D> _ballSprites = new();
    private readonly Dictionary<int, Sprite2D> _splitterSprites = new();
    private readonly Dictionary<int, Sprite2D> _pickupSprites = new();

    // Reused across frames so Render allocates nothing in the hot loop.
    private readonly HashSet<int> _seen = new();
    private readonly List<int> _gone = new();
    private readonly HashSet<int> _seenSplitters = new();
    private readonly List<int> _goneSplitters = new();
    private readonly HashSet<int> _seenPickups = new();
    private readonly List<int> _gonePickups = new();

    // --- Deployed barrier, drawn as THREE region-sliced sprites from one texture.
    // A single sprite stretched across the goal mouth would smear its rounded caps
    // into ellipses — and the caps are the shape's whole point. So the two caps
    // scale uniformly and only the flat middle stretches, which is lossless because
    // the source's middle region has no horizontal variation at all.
    //
    // Source layout (FullBarrier.png, 240x64): cap | middle | cap, and a 2px
    // transparent margin top/bottom so the antialiased edge isn't clipped. The
    // capsule itself is 60px thick, which is why the runtime scale divides by
    // ShieldTexCapsule rather than the full texture height.
    private const float ShieldTexCapW = 32f;
    private const float ShieldTexMidW = 176f;
    private const float ShieldTexH = 64f;
    private const float ShieldTexCapsule = 60f;

    private Sprite2D _shieldLeft = null!;
    private Sprite2D _shieldMid = null!;
    private Sprite2D _shieldRight = null!;

    private GameState? _state;
    private bool _barriersBuilt;

    /// <summary>Build the static sprites from the central
    /// <see cref="SpriteRegistry"/> — the single source of truth for textures, so
    /// nothing here loads a path of its own.</summary>
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

        // The barrier's three slices. Built once and simply hidden while no barrier
        // is deployed, since it is one fixed span that only toggles visibility.
        var shieldTex = sprites.GetTexture(AssetId.FullBarrier);
        _shieldLeft = MakeShieldSlice(shieldTex, new Rect2(0, 0, ShieldTexCapW, ShieldTexH));
        _shieldMid = MakeShieldSlice(shieldTex, new Rect2(ShieldTexCapW, 0, ShieldTexMidW, ShieldTexH));
        _shieldRight = MakeShieldSlice(shieldTex,
            new Rect2(ShieldTexCapW + ShieldTexMidW, 0, ShieldTexCapW, ShieldTexH));
    }

    private Sprite2D MakeShieldSlice(Texture2D tex, Rect2 region)
    {
        var s = new Sprite2D
        {
            Texture = tex,
            RegionEnabled = true,
            RegionRect = region,
            Visible = false,
            // Above the background and corner barriers, alongside the paddle.
            ZIndex = 0,
        };
        AddChild(s);
        return s;
    }

    /// <summary>Position the barrier's three slices across the span the sim collides
    /// against, so the drawn shape and the collider are the same capsule. The caps
    /// keep a uniform scale (staying circular); only the middle stretches.</summary>
    private void RenderShield(GameState state)
    {
        bool active = state.ShieldActive;
        _shieldLeft.Visible = active;
        _shieldMid.Visible = active;
        _shieldRight.Visible = active;
        if (!active)
            return;

        // The sim's radius is the capsule's, so scale off the capsule's own height
        // rather than the texture's (which carries the antialias margin).
        float scale = state.ShieldRadius * 2f / ShieldTexCapsule;
        float capW = ShieldTexCapW * scale;
        float y = state.ShieldY;

        // X0/X1 are the spine's endpoints, i.e. the centres of the two round caps.
        _shieldLeft.Scale = new Vector2(scale, scale);
        _shieldLeft.Position = new Vector2(state.ShieldX0 - capW * 0.5f, y);

        _shieldRight.Scale = new Vector2(scale, scale);
        _shieldRight.Position = new Vector2(state.ShieldX1 + capW * 0.5f, y);

        // The middle fills the gap between the cap sprites, stretched only in x.
        float midW = state.ShieldX1 - state.ShieldX0;
        _shieldMid.Scale = new Vector2(midW / ShieldTexMidW, scale);
        _shieldMid.Position = new Vector2((state.ShieldX0 + state.ShieldX1) * 0.5f, y);
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

    /// <summary>Paint one frame of the field: background, paddle, balls and
    /// splitters, adding and freeing sprites as objects come and go. Purely a view
    /// — it reads <paramref name="state"/> and never writes to it.</summary>
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

        // Loot pickups: same create/move/free-by-id pattern again. The texture comes
        // from the item's own registry row, so a second loot item needs no change
        // here — it just resolves to a different sprite.
        _seenPickups.Clear();
        foreach (var pu in state.Pickups)
        {
            _seenPickups.Add(pu.Id);
            if (!_pickupSprites.TryGetValue(pu.Id, out var sprite))
            {
                sprite = new Sprite2D
                {
                    Texture = SpriteRegistry.Instance.GetTexture(ItemRegistry.Icon(pu.Item)),
                };
                AddChild(sprite);
                _pickupSprites[pu.Id] = sprite;
            }
            sprite.Position = pu.Pos;
            var pusize = sprite.Texture.GetSize();
            if (pusize.X > 0 && pusize.Y > 0)
                sprite.Scale = new Vector2(pu.Radius * 2f / pusize.X, pu.Radius * 2f / pusize.Y);
        }

        _gonePickups.Clear();
        foreach (var id in _pickupSprites.Keys)
            if (!_seenPickups.Contains(id))
                _gonePickups.Add(id);
        foreach (var id in _gonePickups)
        {
            _pickupSprites[id].QueueFree();
            _pickupSprites.Remove(id);
        }

        RenderShield(state);

        QueueRedraw(); // refresh the edge outlines
    }

    /// <summary>Draw the four edge outlines. Vector work only; everything with a
    /// texture is a child sprite positioned in <see cref="Render"/>.</summary>
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

    /// <summary>Outline one edge in the colour of its role — goal, portal or wall —
    /// so a player can see at a glance which sides pass a ball on and which return
    /// it. An unmapped edge falls back to Wall.</summary>
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
