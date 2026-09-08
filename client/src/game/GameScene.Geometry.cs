using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Game;

/// <summary>Static arena geometry resolved once in <c>_Ready</c>: which edge each
/// peer is seated behind, and where the deployed Full Barrier sits between the
/// paddle and the goal line. Pure layout maths — no per-frame state.</summary>
public partial class GameScene
{
    // --- Deployed Full Barrier geometry (see GameState.ShieldX0/X1/Y/Radius) ---

    /// <summary>Gap between the paddle's underside and the top of the barrier, as a
    /// fraction of arena height. Exists so the two read as separate objects rather
    /// than one thick bar.</summary>
    private const float ShieldClearanceHFrac = 6f / 1440f;

    /// <summary>Barrier thickness as a fraction of arena height. Sized to read
    /// clearly without crowding the goal gap, which is 120/1440 tall in total.</summary>
    private const float ShieldThicknessHFrac = 30f / 1440f;

    /// <summary>How far the barrier's rounded ends stop short of the corner barriers,
    /// as a fraction of arena height. Small on purpose: the resulting gap is a few
    /// pixels, far narrower than a ball's ~45px diameter, so the barrier and the
    /// corner triangles seal the goal between them while never overlapping.</summary>
    private const float ShieldEndClearanceHFrac = 4f / 1440f;

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

    /// <summary>Place the deployed barrier: a horizontal capsule in the gap between
    /// the paddle's underside and the goal line, spanning the goal mouth.
    ///
    /// The ends are <b>solved against the corner triangles rather than hardcoded</b>.
    /// Clients run different arena aspect ratios (see docs/planning/known-bugs.md), so
    /// a baked inset would be wrong on every screen but one, and the corner colliders
    /// are the thing the bar must not overlap. Bisecting on the real
    /// <see cref="CornerBarrier.ClosestPoint"/> keeps art and collider derived from the
    /// same geometry — the invariant CornerBarrier already maintains.
    ///
    /// The result leaves a gap of only <see cref="ShieldEndClearanceHFrac"/> at each
    /// end, which is several times narrower than a ball, so the barrier and the corner
    /// triangles seal the goal between them without ever intersecting.</summary>
    private void ResolveShieldGeometry(Vector2 arena)
    {
        float radius = ShieldThicknessHFrac * arena.Y * 0.5f;
        float clearance = ShieldClearanceHFrac * arena.Y;
        float endGap = ShieldEndClearanceHFrac * arena.Y;

        var paddle = _state.Paddle;
        float centreY = paddle.Y + paddle.Height + clearance + radius;
        float needed = radius + endGap;

        // Walk each end inward to the first x that clears the corner triangles.
        // Monotonic in x over each half (the hypotenuse recedes as you move inward),
        // so a plain bisection converges.
        float left = Bisect(0f, arena.X * 0.5f, centreY, needed, fromLeft: true);
        float right = Bisect(arena.X * 0.5f, arena.X, centreY, needed, fromLeft: false);

        _state.ShieldRadius = radius;
        _state.ShieldY = centreY;
        _state.ShieldX0 = left;
        _state.ShieldX1 = right;

        Log.Debug("game.loot",
            $"shield span x {left:F0}..{right:F0} y {centreY:F0} r {radius:F1} " +
            $"({100f * (right - left) / arena.X:F1}% of width)");
    }

    // Smallest (fromLeft) or largest (!fromLeft) x in [lo,hi] whose distance to every
    // corner triangle is at least `needed`. Falls back to the inner bound if no such
    // x exists, which can only happen on a degenerately small arena.
    private float Bisect(float lo, float hi, float y, float needed, bool fromLeft)
    {
        for (int i = 0; i < 40; i++)
        {
            float mid = (lo + hi) * 0.5f;
            bool clear = DistanceToCorners(new Vector2(mid, y)) >= needed;
            if (clear == fromLeft)
                hi = mid;
            else
                lo = mid;
        }
        return fromLeft ? hi : lo;
    }

    /// <summary>Distance from a point to the nearest corner barrier, using the same
    /// closest-point routine the ball collision uses.</summary>
    private float DistanceToCorners(Vector2 p)
    {
        float best = float.MaxValue;
        foreach (var tri in _state.Barriers)
            best = Mathf.Min(best, p.DistanceTo(CornerBarrier.ClosestPoint(p, in tri)));
        return best;
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
