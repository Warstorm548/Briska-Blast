using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// Static geometry for the four corner barriers — a single source of truth shared by
/// the simulation (which reads the collision triangles) and the view (which places the
/// sprites), so the collider can never drift from the art.
///
/// The <c>Cornerbarrier</c> sprite is a right triangle drawn for the BOTTOM-LEFT corner:
/// the right angle sits on its very bottom-left pixel, the two legs run up the left edge
/// and along the bottom edge, and the hypotenuse (local <c>(0,0)→(TexW,TexH)</c>) faces the
/// arena centre. The other three corners are the SAME sprite rotated in 90° steps about
/// that bottom-left pixel, with the pixel pinned to the screen corner, so every hypotenuse
/// opens toward the centre and turns shots away from the goal corners. The barrier is sized
/// as a fraction of arena HEIGHT (like the paddle / ball / goal-gap tuning) so every
/// resolution gets the same proportion.
/// </summary>
public static class CornerBarrier
{
    /// <summary>Native sprite size the local footprint below was measured against.</summary>
    public const float TexW = 100f;
    public const float TexH = 120f;

    /// <summary>Barrier height as a fraction of arena height (renders at native design
    /// size on a 1440-tall arena). The single knob for the barrier's overall size.</summary>
    public const float HeightHFrac = 120f / 1440f;

    /// <summary>Collision-surface inset, in sprite-local (design) pixels: the collider sits
    /// this far INSIDE the opaque art on every edge, so a ball overlaps the sprite slightly
    /// before it bounces.</summary>
    private const float InsetPx = 1f;

    // The solid right-triangle footprint in sprite-local pixels (texture space: origin
    // top-left, y DOWN): right angle on the bottom-left pixel, hypotenuse (0,0)→(TexW,TexH)
    // facing the arena. Insetting all three edges 1px keeps the two legs a hair off the walls
    // and pulls the hypotenuse 1px into the solid.
    private static readonly Vector2[] LocalTri =
    {
        new Vector2(0f, 0f),     // apex, up the left wall
        new Vector2(0f, TexH),   // bottom-left pixel (pinned to the screen corner)
        new Vector2(TexW, TexH), // along the bottom wall
    };

    /// <summary>A solid barrier triangle in arena-pixel coordinates. Vertex winding is
    /// irrelevant — collision uses unsigned distance and same-sign side tests.</summary>
    public readonly struct BarrierTri
    {
        public readonly Vector2 A, B, C;
        public BarrierTri(Vector2 a, Vector2 b, Vector2 c) { A = a; B = b; C = c; }
    }

    public enum Corner { BottomLeft, TopLeft, TopRight, BottomRight }

    /// <summary>Each corner with the clockwise rotation applied to the as-drawn
    /// (bottom-left) sprite. Order/orientation matches the approved layout.</summary>
    public static readonly (Corner Corner, float Rotation)[] Corners =
    {
        (Corner.BottomLeft,  0f),
        (Corner.TopLeft,     Mathf.Pi / 2f),
        (Corner.TopRight,    Mathf.Pi),
        (Corner.BottomRight, 3f * Mathf.Pi / 2f),
    };

    /// <summary>Screen point the sprite's bottom-left pixel pins to for a corner.</summary>
    public static Vector2 Pivot(Corner c, float w, float h) => c switch
    {
        Corner.BottomLeft => new Vector2(0, h),
        Corner.TopLeft => new Vector2(0, 0),
        Corner.TopRight => new Vector2(w, 0),
        _ => new Vector2(w, h), // BottomRight
    };

    /// <summary>Uniform, aspect-preserving scale that renders the sprite at
    /// <see cref="HeightHFrac"/> of the arena height.</summary>
    public static float ScaleFor(float arenaHeight) => HeightHFrac * arenaHeight / TexH;

    /// <summary>Append the four corner collision triangles in arena-pixel coordinates. Each
    /// is the 1px-inset local triangle mapped through the SAME transform the view uses to
    /// place its sprite — <c>world = pivot + Rot(θ)·scale·(local − bottomLeftPixel)</c> — so
    /// art and collider can never drift.</summary>
    public static void AppendTriangles(List<BarrierTri> into, float w, float h)
    {
        float s = ScaleFor(h);
        var blPixel = new Vector2(0f, TexH); // bottom-left pixel in texture space
        var tri = InsetTriangle(LocalTri[0], LocalTri[1], LocalTri[2], InsetPx);

        foreach (var (corner, rot) in Corners)
        {
            var xf = new Transform2D(rot, Pivot(corner, w, h));
            into.Add(new BarrierTri(
                xf * ((tri[0] - blPixel) * s),
                xf * ((tri[1] - blPixel) * s),
                xf * ((tri[2] - blPixel) * s)));
        }
    }

    // Shrink a triangle by pushing every edge inward by d pixels. For a triangle that is a
    // uniform scale of (r−d)/r about the incenter: each side lies exactly r (the inradius)
    // from the incenter, so moving it in by d puts it at r−d while keeping edge directions.
    private static Vector2[] InsetTriangle(Vector2 a, Vector2 b, Vector2 c, float d)
    {
        float la = b.DistanceTo(c); // side opposite a
        float lb = c.DistanceTo(a); // side opposite b
        float lc = a.DistanceTo(b); // side opposite c
        float peri = la + lb + lc;
        var incenter = (la * a + lb * b + lc * c) / peri;
        float area = 0.5f * Mathf.Abs((b.X - a.X) * (c.Y - a.Y) - (c.X - a.X) * (b.Y - a.Y));
        float inradius = area / (0.5f * peri);
        float k = (inradius - d) / inradius;
        return new[]
        {
            incenter + (a - incenter) * k,
            incenter + (b - incenter) * k,
            incenter + (c - incenter) * k,
        };
    }

    /// <summary>True when a circle (centre <paramref name="c"/>, radius <paramref name="r"/>)
    /// touches the solid triangle — centre inside, or nearest boundary point within r.</summary>
    public static bool Overlaps(in BarrierTri t, Vector2 c, float r)
    {
        if (PointInside(c, in t))
            return true;
        return c.DistanceSquaredTo(ClosestPoint(c, in t)) < r * r;
    }

    /// <summary>2D point-in-triangle via consistent cross-product signs (edges inclusive).</summary>
    public static bool PointInside(Vector2 p, in BarrierTri t)
    {
        float d1 = Cross(p, t.A, t.B);
        float d2 = Cross(p, t.B, t.C);
        float d3 = Cross(p, t.C, t.A);
        bool hasNeg = d1 < 0f || d2 < 0f || d3 < 0f;
        bool hasPos = d1 > 0f || d2 > 0f || d3 > 0f;
        return !(hasNeg && hasPos);
    }

    // Twice the signed area of (a, b, p): its sign tells which side of edge a→b p lies on.
    private static float Cross(Vector2 p, Vector2 a, Vector2 b) =>
        (b.X - a.X) * (p.Y - a.Y) - (b.Y - a.Y) * (p.X - a.X);

    /// <summary>Closest point on the (solid) triangle to <paramref name="p"/> — Ericson,
    /// Real-Time Collision Detection §5.1.5. Returns <paramref name="p"/> itself when inside.</summary>
    public static Vector2 ClosestPoint(Vector2 p, in BarrierTri t)
    {
        Vector2 a = t.A, b = t.B, c = t.C;
        Vector2 ab = b - a, ac = c - a, ap = p - a;
        float d1 = ab.Dot(ap), d2 = ac.Dot(ap);
        if (d1 <= 0f && d2 <= 0f) return a;

        Vector2 bp = p - b;
        float d3 = ab.Dot(bp), d4 = ac.Dot(bp);
        if (d3 >= 0f && d4 <= d3) return b;

        float vc = d1 * d4 - d3 * d2;
        if (vc <= 0f && d1 >= 0f && d3 <= 0f)
            return a + ab * (d1 / (d1 - d3));

        Vector2 cp = p - c;
        float d5 = ab.Dot(cp), d6 = ac.Dot(cp);
        if (d6 >= 0f && d5 <= d6) return c;

        float vb = d5 * d2 - d1 * d6;
        if (vb <= 0f && d2 >= 0f && d6 <= 0f)
            return a + ac * (d2 / (d2 - d6));

        float va = d3 * d6 - d5 * d4;
        if (va <= 0f && (d4 - d3) >= 0f && (d5 - d6) >= 0f)
            return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));

        float denom = 1f / (va + vb + vc);
        return a + ab * (vb * denom) + ac * (vc * denom);
    }

    /// <summary>For a centre already INSIDE the solid, the shallowest way out: the outward
    /// unit normal of the nearest edge and how deep the centre sits past it. Used to push a
    /// deeply-penetrated ball back out.</summary>
    public static (Vector2 Normal, float Depth) NearestEdgeExit(Vector2 p, in BarrierTri t)
    {
        var best = EdgeExit(p, t.A, t.B, t.C);
        var e2 = EdgeExit(p, t.B, t.C, t.A);
        if (e2.Depth < best.Depth) best = e2;
        var e3 = EdgeExit(p, t.C, t.A, t.B);
        if (e3.Depth < best.Depth) best = e3;
        return best;
    }

    // Outward unit normal + penetration depth of p relative to edge (e0→e1). `opp` is the
    // third vertex, used only to orient the normal away from the triangle interior.
    private static (Vector2 Normal, float Depth) EdgeExit(Vector2 p, Vector2 e0, Vector2 e1, Vector2 opp)
    {
        var edge = e1 - e0;
        var n = new Vector2(edge.Y, -edge.X).Normalized(); // a perpendicular to the edge
        if ((opp - e0).Dot(n) > 0f)
            n = -n; // flip so n points away from the interior
        float depth = (e0 - p).Dot(n); // p inside → distance out to this edge, ≥ 0
        return (n, depth);
    }
}
