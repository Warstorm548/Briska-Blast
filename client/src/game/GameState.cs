using System.Collections.Generic;
using BriskaBlast.Core;
using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// The four edges of a player's own screen, in the local (always-upright) frame.
/// <see cref="Bottom"/> is always this player's goal (the paddle sits above it);
/// the other three are portals to peers or solid walls. See
/// docs/planning/multiplayer-client-stages.md (Stage 3, Extended mode).
/// </summary>
public enum Edge
{
    Top,
    Right,
    Bottom,
    Left,
}

/// <summary>What lies across an edge.</summary>
public enum EdgeKind
{
    /// <summary>Solid boundary — the ball reflects (no peer occupies this slot).</summary>
    Wall,
    /// <summary>This player's goal line. The ball passing it (past the paddle)
    /// ends the rally and scores for the ball's last hitter.</summary>
    Goal,
    /// <summary>Shared edge with a peer — the ball crossing it is handed off to
    /// that peer (see <see cref="EdgeTarget.PeerId"/>).</summary>
    Portal,
}

/// <summary>What an edge connects to. <see cref="PeerId"/> is meaningful only
/// when <see cref="Kind"/> is <see cref="EdgeKind.Portal"/>.</summary>
public readonly struct EdgeTarget
{
    public readonly EdgeKind Kind;
    public readonly string PeerId;

    /// <summary>Private so an edge can only be built through <see cref="Wall"/>,
    /// <see cref="Goal"/> or <see cref="Portal"/> — the three shapes that exist.
    /// A Wall or Goal with a peer id, or a Portal without one, is not
    /// representable.</summary>
    private EdgeTarget(EdgeKind kind, string peerId)
    {
        Kind = kind;
        PeerId = peerId;
    }

    /// <summary>An edge the ball bounces off. Carries no peer.</summary>
    public static readonly EdgeTarget Wall = new(EdgeKind.Wall, "");
    /// <summary>An edge the ball scores through. Always the bottom edge, and
    /// always this screen's own goal.</summary>
    public static readonly EdgeTarget Goal = new(EdgeKind.Goal, "");
    /// <summary>An edge the ball is handed across, to the screen belonging to
    /// <paramref name="peerId"/>. See <c>docs/architecture/extended-mode.md</c>
    /// for the handoff itself.</summary>
    public static EdgeTarget Portal(string peerId) => new(EdgeKind.Portal, peerId);
}

/// <summary>What kind of ball this is. The white master/starter ball is re-served
/// when it's lost and worth a single point; <see cref="Split"/> balls are the
/// BallBT bonus balls spawned by a ball splitter — worth double and simply removed
/// (never re-served) when they reach a goal. Drives texture, score weight and
/// re-serve behaviour.</summary>
public enum BallKind
{
    Master,
    Split,
}

/// <summary>
/// A ball in flight on this screen. Plain data; the simulation integrates it and
/// the view draws it. <see cref="LastHitterId"/> travels with the ball across
/// handoffs so whoever's goal it eventually enters can credit the right scorer.
/// </summary>
public sealed class Ball
{
    public int Id;
    public Vector2 Pos;
    public Vector2 Vel;
    public float Radius = 16f;
    /// <summary>player_id of the last player to deflect this ball with a paddle;
    /// empty until someone has hit it.</summary>
    public string LastHitterId = "";
    /// <summary>Master (the white starter ball) by default; <see cref="BallKind.Split"/>
    /// for BallBT balls produced by a splitter.</summary>
    public BallKind Kind = BallKind.Master;
}

/// <summary>A ball-splitter element (BallSpliter) sitting on this screen. A system
/// spawn: it appears on a host-configured cadence, and when a ball touches it the
/// splitter is consumed and spits out three BallBT split balls. Local to this
/// screen — splitters are never handed off; each client spawns its own.</summary>
public sealed class Splitter
{
    public int Id;
    public Vector2 Pos;
    public float Radius = 24f;
}

/// <summary>A loot item lying in the arena, waiting to be collected. A system spawn
/// like <see cref="Splitter"/>: it appears on a host-configured cadence and is
/// consumed when a ball touches it.
///
/// Unlike a splitter, collecting one does NOT necessarily benefit this screen — the
/// item goes to the ball's last hitter, who is usually a peer. The pickup itself is
/// still purely local (each client spawns its own); only the award crosses the wire.
/// See <c>GameSimulation.ResolvePickups</c>.</summary>
public sealed class Pickup
{
    public int Id;
    public Vector2 Pos;
    public float Radius = 24f;
    /// <summary>Which item this grants. Drives both the sprite and what lands in
    /// the collector's hotbar.</summary>
    public ItemId Item;
}

/// <summary>The local player's paddle: a horizontal bar that slides along the
/// bottom of the screen, a gap above the goal line.</summary>
public sealed class Paddle
{
    /// <summary>Center x of the paddle. Clamped to the arena by the simulation.</summary>
    public float CenterX;
    public float Width = 160f;
    public float Height = 24f;
    /// <summary>Top y of the paddle face the ball bounces off. Set from arena
    /// height minus the goal gap when the state is built.</summary>
    public float Y;
}

/// <summary>One hotbar slot: an actionable item the player holds, or nothing.
/// Empty is the normal state today — no items exist yet, so every slot starts and
/// stays empty until an item system fills one.</summary>
public sealed class ItemSlot
{
    /// <summary>Sprite drawn in the slot's inner square, or null when the slot is
    /// empty. Reuses <see cref="AssetId"/> rather than inventing an item enum: when
    /// real items arrive this becomes an item id whose lookup row carries the icon.</summary>
    public AssetId? Icon;

    /// <summary>How many of the item are held. Maximum stack size will differ per item
    /// and belongs on the future item lookup table (the <see cref="SpriteRegistry"/>
    /// pattern), not here — a slot should not decide its own cap.</summary>
    public int Count;

    /// <summary>Nothing to draw and nothing to activate.</summary>
    public bool IsEmpty => Icon == null;
}

/// <summary>
/// The local player's action bar: a fixed row of slots below the play field, each
/// fired by its own number key. Fixed-length and allocated once — slots are emptied
/// and refilled in place, never added or removed, so the view can bind one UI node
/// per slot at build time and never rebuild the row.
/// </summary>
public sealed class Hotbar
{
    /// <summary>Slots in the bar, left to right. Bound to keys 1..<see cref="SlotCount"/>.
    /// Raising this grows the row and the model together; the view sizes itself from it
    /// too, so the only other thing a bigger bar needs is more input actions.</summary>
    public const int SlotCount = 5;

    /// <summary>The slots themselves, left to right, always
    /// <see cref="SlotCount"/> long and never containing a null — see
    /// <see cref="CreateSlots"/>.</summary>
    public readonly ItemSlot[] Slots = CreateSlots();

    /// <summary>Fill the bar with empty slots. A field initialiser cannot loop, and
    /// leaving the array's entries null would put a null check on every hotbar read
    /// instead of an empty slot here.</summary>
    private static ItemSlot[] CreateSlots()
    {
        var slots = new ItemSlot[SlotCount];
        for (int i = 0; i < slots.Length; i++)
            slots[i] = new ItemSlot();
        return slots;
    }

    /// <summary>Take one of <paramref name="item"/> into the bar: stack it onto a
    /// slot that already holds it and has room, else claim the first empty slot.
    /// Returns false when the bar has no room — every slot full or already at this
    /// item's <see cref="ItemRegistry"/> cap — in which case nothing changes.
    ///
    /// Stacking is tried before claiming an empty slot so a player never ends up
    /// holding the same item in two slots while a third sits empty.</summary>
    public bool TryAdd(ItemId item)
    {
        var icon = ItemRegistry.Icon(item);
        int max = ItemRegistry.MaxStack(item);

        foreach (var slot in Slots)
            if (!slot.IsEmpty && slot.Icon == icon && slot.Count < max)
            {
                slot.Count++;
                return true;
            }

        foreach (var slot in Slots)
            if (slot.IsEmpty)
            {
                slot.Icon = icon;
                slot.Count = 1;
                return true;
            }

        return false;
    }

    /// <summary>Spend one charge from a slot. When that was the last one the slot is
    /// <b>cleared outright</b> — icon and all — rather than left showing a zero, so
    /// the next pickup of ANY item can claim it. Slots are not owned by an item.
    ///
    /// Returns the item that was spent, or null when the slot was already empty.
    /// Note the caller must not read the slot's icon afterwards to learn what it
    /// activated; that is why the item comes back from here.</summary>
    public ItemId? Consume(int index)
    {
        if (index < 0 || index >= SlotCount)
            return null;

        var slot = Slots[index];
        if (slot.IsEmpty || slot.Count <= 0)
            return null;
        if (!ItemRegistry.TryFromIcon(slot.Icon!.Value, out var item))
            return null;

        slot.Count--;
        if (slot.Count <= 0)
        {
            slot.Icon = null;
            slot.Count = 0;
        }
        return item;
    }
}

/// <summary>
/// All authoritative state for THIS player's screen. There is no shared arena:
/// each client owns and renders only its own <see cref="GameState"/>. The
/// simulation mutates it, the view observes it, and the net layer writes peer
/// handoffs / server score updates into it.
/// </summary>
public sealed class GameState
{
    /// <summary>Play-field size in world units. Derived from the viewport minus the
    /// action-bar strip, and identical on every client in a match — the ball's
    /// position is exchanged in these units, so the screens must agree.</summary>
    public float ArenaWidth;

    /// <inheritdoc cref="ArenaWidth"/>
    public float ArenaHeight;

    /// <summary>This screen's own paddle. Only the local player moves it; the other
    /// screens' paddles are never modelled here, since all that crosses the mesh is
    /// the ball.</summary>
    public readonly Paddle Paddle = new();

    /// <summary>The local action bar. Local-only and never networked — what a player
    /// holds is their own business until an item actually does something.</summary>
    public readonly Hotbar Hotbar = new();

    /// <summary>Balls currently on THIS screen. A ball handed across a portal is
    /// removed here and added on the receiving screen, so exactly one screen owns
    /// any given ball at a time.</summary>
    public readonly List<Ball> Balls = new();

    /// <summary>System-spawned ball splitters currently on this screen (local-only;
    /// never networked). A ball touching one consumes it into three split balls.</summary>
    public readonly List<Splitter> Splitters = new();

    /// <summary>Loot items lying on THIS screen waiting to be collected (local-only;
    /// each client spawns its own). A ball touching one consumes it and awards the
    /// item to that ball's last hitter — who is often a peer, which is the one part
    /// of the loot system that leaves this screen.</summary>
    public readonly List<Pickup> Pickups = new();

    /// <summary>Seconds left on the local player's deployed Full Barrier; 0 when no
    /// barrier is up. Activating the item ADDS to this rather than replacing it, so
    /// a second activation at 15s remaining leaves 45s.
    ///
    /// Deliberately not stored on the hotbar slot: spending the last charge clears
    /// the slot, and the countdown has to outlive that — the player is still standing
    /// behind the barrier they just paid for.</summary>
    public float ShieldSecsRemaining;

    /// <summary>Whether a barrier is currently deployed. The simulation only collides
    /// balls against the barrier while this holds.</summary>
    public bool ShieldActive => ShieldSecsRemaining > 0f;

    /// <summary>Geometry of the deployed barrier: a capsule whose spine runs
    /// horizontally from (<see cref="ShieldX0"/>, <see cref="ShieldY"/>) to
    /// (<see cref="ShieldX1"/>, <see cref="ShieldY"/>) with radius
    /// <see cref="ShieldRadius"/> — which is exactly "a rectangle with half-round
    /// ends". Resolved once from the arena in <c>GameScene._Ready</c>; the ends are
    /// pulled in far enough to clear the corner barriers, so the bar never overlaps
    /// them while the gap it leaves stays far narrower than a ball.</summary>
    public float ShieldX0, ShieldX1, ShieldY, ShieldRadius;

    /// <summary>When true, BallBT split balls that hit another splitter split again;
    /// when false only the master ball can trigger a split. Host-configured.</summary>
    public bool ChainSplitEnabled = true;

    /// <summary>Solid corner-barrier collision triangles in arena pixels (one right triangle
    /// per corner, 4 total). Static local geometry like the wall edges — balls bounce off them
    /// but never hand off across them, so they're never networked. Built once in
    /// <c>GameScene._Ready</c> via <see cref="CornerBarrier.AppendTriangles"/>.</summary>
    public readonly List<CornerBarrier.BarrierTri> Barriers = new();

    /// <summary>Per-edge mapping for this screen. Bottom is always
    /// <see cref="EdgeTarget.Goal"/>; Top/Right/Left are Portal or Wall.</summary>
    public readonly Dictionary<Edge, EdgeTarget> Edges = new();

    /// <summary>Mirror of the server-authoritative tally (player_id → points).
    /// Overwritten wholesale on each ScoreUpdate — never incremented locally.</summary>
    public readonly Dictionary<string, int> Scores = new();

    /// <summary>Which observed tally each player's CURRENT score first appeared
    /// in, as a <see cref="_scoreSeq"/> reading. The leaderboard breaks ties on
    /// it — equal scores rank by who got there first.
    ///
    /// A COUNT of tallies, deliberately not a clock reading. Ordering has to
    /// agree on every screen, and the only thing every client shares is the
    /// sequence of broadcasts it applies — not its timing. A millisecond clock
    /// looks equivalent and is not: two frames drained in one engine tick read
    /// the same millisecond on a machine quick enough to do it, and different
    /// ones on a machine that is not, so the tie-break would fall to seat order
    /// on the first screen and to arrival order on the second. The counter moves
    /// exactly once per tally, so the Nth tally stamps N everywhere.</summary>
    public readonly Dictionary<string, ulong> ScoreReachedAtSeq = new();

    // Tallies applied so far. The stamp source — see ScoreReachedAtSeq for why
    // this is a count and not a clock. Starts at 0, so 0 reads as "not seen in
    // any tally yet" and never collides with a real stamp.
    private ulong _scoreSeq;

    // Scratch for ApplyScores. ScoreReachedAtSeq is readonly, so a rebuild has
    // to stage the new stamps somewhere before clearing the old ones.
    private readonly Dictionary<string, ulong> _stamps = new();

    /// <summary>
    /// Adopt a server tally wholesale, stamping every value that changed.
    ///
    /// The one place scores are written. Both callers used to clear and refill
    /// the map separately; keeping the stamps honest in two places would have
    /// been one edit away from silently reordering the leaderboard.
    ///
    /// A value is stamped whenever it differs from what is held — a DECREASE
    /// included, since the server is authoritative and "changed" is the only
    /// thing worth marking. Players whose score did not move keep their original
    /// stamp, which is what makes the tie-break mean "who got here first" rather
    /// than "who was in the last packet". Anyone the server no longer lists
    /// falls out, so a stale stamp cannot outlive its score.
    ///
    /// Everything that moves in ONE tally shares that tally's number. The frame
    /// says only what the scores now are, never who just scored, so when two
    /// players move together there is nothing to separate them and the
    /// leaderboard's seat-order key does it — the same way on every screen.
    ///
    /// The stamps count tallies THIS client applied, so they are comparable only
    /// against each other. A client that joined late, or missed frames, starts
    /// counting where it came in and collapses everything before that into its
    /// first tally; its board can therefore break a tie differently from one that
    /// saw the whole match. Nothing on the wire carries the history needed to fix
    /// that, and seat order still keeps every board internally stable.
    /// </summary>
    public void ApplyScores(IReadOnlyDictionary<string, int> scores)
    {
        ulong seq = ++_scoreSeq;

        _stamps.Clear();
        foreach (var (pid, pts) in scores)
        {
            bool moved = !Scores.TryGetValue(pid, out var held) || held != pts;
            _stamps[pid] = !moved && ScoreReachedAtSeq.TryGetValue(pid, out var at) ? at : seq;
        }

        ScoreReachedAtSeq.Clear();
        foreach (var (pid, at) in _stamps)
            ScoreReachedAtSeq[pid] = at;

        Scores.Clear();
        foreach (var (pid, pts) in scores)
            Scores[pid] = pts;
    }

    public string LocalPlayerId = "";

    /// <summary>Per-seat stride that namespaces ball ids so balls created
    /// independently on different screens (split balls, simultaneous serves) never
    /// collide once handed across the mesh. Each screen's <see cref="BallIdBase"/>
    /// is its seat × this; a screen will never allocate near this many balls.</summary>
    public const int BallIdSeatStride = 1_000_000;

    /// <summary>Range of the per-process id offset folded into <see cref="BallIdBase"/>
    /// so a process-death rejoin (which restarts the local counter at 0) begins in a
    /// different sub-range of its seat block. Kept below the stride so the offset plus
    /// a match's ball count stays within the seat's block.</summary>
    public const int BallIdRejoinOffsetRange = 900_000;

    /// <summary>Start of this screen's ball-id block (seat × <see cref="BallIdSeatStride"/>),
    /// set once from the seating in GameScene. Defaults to 0 (seat 0 / unseated).</summary>
    public int BallIdBase;

    private int _nextBallId;

    /// <summary>Globally-unique ball id allocator. Monotonic within this screen's
    /// id block; handed-off balls keep the id assigned by their origin screen, so
    /// ids stay unique across the mesh.</summary>
    public int NextBallId() => BallIdBase + _nextBallId++;

    private int _nextSplitterId;

    /// <summary>Splitter id allocator. Splitters never leave this screen, so a plain
    /// local counter suffices (no cross-screen uniqueness needed).</summary>
    public int NextSplitterId() => _nextSplitterId++;

    private int _nextPickupId;

    /// <summary>Pickup id allocator. Like splitter ids, purely local: a pickup is
    /// only ever addressed on the screen that spawned it (the award that leaves this
    /// screen carries an item id, not a pickup id), so no seat namespacing.</summary>
    public int NextPickupId() => _nextPickupId++;
}
