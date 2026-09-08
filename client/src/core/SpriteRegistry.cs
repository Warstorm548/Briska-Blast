using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Core;

/// <summary>
/// Who drives an asset: a player (paddles, the served ball), the game itself as a
/// static fixture (corner barriers), the game itself as a random spawn (the ball
/// splitter), or nobody at all because it is screen furniture (the hotbar slot frame).
/// Lets systems treat these groups uniformly — e.g. the host spawn-frequency
/// settings enumerate only <see cref="SystemHandled"/>.
/// </summary>
public enum AssetCategory
{
    /// <summary>Driven by a player — paddles, the served ball.</summary>
    PlayerControlled,
    /// <summary>Owned by the game as a STATIC fixture with no spawn cadence — the corner
    /// barriers. Distinct from <see cref="SystemHandled"/> so it never appears in the
    /// host's Random-Spawns frequency UI. Room to grow later (e.g. a mechanic that hides
    /// a barrier for a particular player).</summary>
    SystemControlled,
    /// <summary>Spawned by the game on a host-tunable cadence — the ball splitter and
    /// future random-spawn elements. Enumerated by <see cref="SpriteRegistry.SystemSpawns"/>.</summary>
    SystemHandled,
    /// <summary>Screen furniture, not a world object — the hotbar slot frame and future
    /// HUD chrome. Never spawned, never simulated, never collided with; it exists only to
    /// be drawn on a <see cref="Godot.CanvasLayer"/>. The other three values all answer
    /// "who moves this thing in the arena", which no UI sprite has an answer to.</summary>
    Ui,
}

/// <summary>
/// Stable lookup id for every sprite that needs fast runtime resolution. Numbers
/// count upward and never change once assigned (safe to persist or send over the
/// wire); register a new sprite by adding the next free number here and a matching
/// row in <see cref="SpriteRegistry"/>.
/// </summary>
public enum AssetId
{
    MasterBall = 1,
    BallBT = 2,
    BallSpliter = 3,
    Paddle = 4,
    Background = 5,
    CornerBarrier = 6,
    /// <summary>The hotbar slot frame. Named for the concept, not the art revision —
    /// the current sprite is <c>ItemSlotV3.png</c> and swapping it again is a path
    /// change on the <see cref="SpriteRegistry"/> row, not a new id.</summary>
    ItemSlot = 7,
    /// <summary>The Full Barrier loot item — used BOTH as the collectible lying in
    /// the arena and as its hotbar slot icon, so what a player picks up looks like
    /// what lands in their bar. Placeholder art.</summary>
    BarrierShield = 8,
    /// <summary>The deployed Full Barrier itself: a neon capsule spanning the goal
    /// mouth below the paddle. Drawn from three regions of one sprite so its rounded
    /// caps keep their shape at any span — see <c>View2D</c>.</summary>
    FullBarrier = 9,
}

/// <summary>One row of the asset lookup table: a stable id, a human label, the
/// <c>res://</c> path, and the controlling category.</summary>
public readonly struct AssetEntry
{
    public readonly AssetId Id;
    public readonly string Name;
    public readonly string ResPath;
    public readonly AssetCategory Category;

    public AssetEntry(AssetId id, string name, string resPath, AssetCategory category)
    {
        Id = id;
        Name = name;
        ResPath = resPath;
        Category = category;
    }
}

/// <summary>
/// Central asset lookup table (autoload singleton, registered first in
/// project.godot). Maps a stable <see cref="AssetId"/> to its resource path and
/// category, and lazily loads + caches the <see cref="Texture2D"/>. This is the
/// single source of truth for sprite textures — it replaces the scattered path
/// constants that used to live in the view.
/// </summary>
public partial class SpriteRegistry : Node
{
    public static SpriteRegistry Instance { get; private set; } = null!;

    /// <summary>The lookup "grid": one row per sprite, in id order. Add a new
    /// sprite by giving it the next <see cref="AssetId"/> and appending a row.</summary>
    private static readonly AssetEntry[] Entries =
    {
        new(AssetId.MasterBall, "MasterBall", "res://src/assets/Starter balls/Ball.png", AssetCategory.PlayerControlled),
        new(AssetId.BallBT, "BallBT", "res://src/assets/sprites/Balls/BallBT.png", AssetCategory.PlayerControlled),
        new(AssetId.BallSpliter, "BallSpliter", "res://src/assets/sprites/RandomSpawns/BallSpliter.png", AssetCategory.SystemHandled),
        new(AssetId.Paddle, "Paddle", "res://src/assets/Paddles/BallStricker.png", AssetCategory.PlayerControlled),
        new(AssetId.Background, "Background", "res://src/assets/sprites/backgrounds/BackgroundDefault.png", AssetCategory.PlayerControlled),
        new(AssetId.CornerBarrier, "CornerBarrier", "res://src/assets/sprites/Platforms/Cornerbarrier.png", AssetCategory.SystemControlled),
        new(AssetId.ItemSlot, "ItemSlot", "res://src/assets/sprites/ActionBarArea/ItemSlotV3.png", AssetCategory.Ui),
        new(AssetId.BarrierShield, "BarrierShield", "res://src/assets/sprites/loottable/BarrierShieldIcon.png", AssetCategory.SystemHandled),
        new(AssetId.FullBarrier, "FullBarrier", "res://src/assets/sprites/loottable/FullBarrier.png", AssetCategory.SystemControlled),
    };

    private readonly Dictionary<AssetId, AssetEntry> _byId = new();
    private readonly Dictionary<AssetId, Texture2D> _texCache = new();

    public override void _Ready()
    {
        Instance = this;
        foreach (var e in Entries)
            _byId[e.Id] = e;
    }

    /// <summary>The lookup-table row for an id.</summary>
    public AssetEntry Get(AssetId id) => _byId[id];

    public AssetCategory GetCategory(AssetId id) => _byId[id].Category;

    /// <summary>Lazily loads (and caches) the texture for an asset.</summary>
    public Texture2D GetTexture(AssetId id)
    {
        if (_texCache.TryGetValue(id, out var tex))
            return tex;
        tex = GD.Load<Texture2D>(_byId[id].ResPath);
        _texCache[id] = tex;
        return tex;
    }

    /// <summary>Every system-handled (game-spawned) asset — e.g. the random-spawn
    /// elements the host can tune a spawn frequency for.</summary>
    public IEnumerable<AssetEntry> SystemSpawns()
    {
        foreach (var e in Entries)
            if (e.Category == AssetCategory.SystemHandled)
                yield return e;
    }
}
