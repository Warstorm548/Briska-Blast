using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// Frame-independent ball handoff math. A ball leaving one screen and entering
/// another can't share a coordinate frame — each player renders its own screen
/// upright and assigns edges to peers independently. So an exit is reduced to a
/// canonical triple relative to its edge:
/// <list type="bullet">
/// <item><c>Perp</c> — speed leaving through the edge (outward normal).</item>
/// <item><c>Tang</c> — speed along the edge (tangent direction).</item>
/// <item><c>Along</c> — position along the edge in [0,1].</item>
/// </list>
/// The receiver recomposes the same triple onto its own entry edge with the
/// perpendicular component pointing inward. Speed is preserved
/// (<c>Perp² + Tang²</c> is invariant), so the canonical form is all the wire
/// packet (Slice C) needs to carry — no edge orientation crosses the wire.
/// </summary>
public static class BallTransform
{
    /// <summary>Inward (into-the-arena) unit normal of an edge, local frame
    /// (+x right, +y down).</summary>
    public static Vector2 InwardNormal(Edge edge) => edge switch
    {
        Edge.Top => new Vector2(0, 1),
        Edge.Bottom => new Vector2(0, -1),
        Edge.Left => new Vector2(1, 0),
        Edge.Right => new Vector2(-1, 0),
        _ => Vector2.Zero,
    };

    /// <summary>Unit tangent in the direction of increasing <c>Along</c>
    /// (x for Top/Bottom, y for Left/Right).</summary>
    public static Vector2 Tangent(Edge edge) => edge switch
    {
        Edge.Top or Edge.Bottom => new Vector2(1, 0),
        Edge.Left or Edge.Right => new Vector2(0, 1),
        _ => Vector2.Zero,
    };

    /// <summary>Point on an edge at fraction <paramref name="along"/> ∈ [0,1].</summary>
    public static Vector2 PointOnEdge(Edge edge, float along, float w, float h) => edge switch
    {
        Edge.Top => new Vector2(along * w, 0),
        Edge.Bottom => new Vector2(along * w, h),
        Edge.Left => new Vector2(0, along * h),
        Edge.Right => new Vector2(w, along * h),
        _ => Vector2.Zero,
    };

    /// <summary>Decompose an outgoing velocity on <paramref name="exitEdge"/> into
    /// the canonical (perpendicular-out, tangential) speeds.</summary>
    public static (float Perp, float Tang) ToCanonical(Edge exitEdge, Vector2 velocity)
    {
        Vector2 outward = -InwardNormal(exitEdge);
        return (velocity.Dot(outward), velocity.Dot(Tangent(exitEdge)));
    }

    /// <summary>Recompose a canonical exit as an entry on <paramref name="entryEdge"/>:
    /// the perpendicular speed now points inward, the position is nudged inside by
    /// <paramref name="radius"/> so the ball starts clear of the edge.</summary>
    public static (Vector2 Pos, Vector2 Vel) FromCanonical(
        Edge entryEdge, float perp, float tang, float along, float w, float h, float radius)
    {
        Vector2 inward = InwardNormal(entryEdge);
        Vector2 vel = inward * perp + Tangent(entryEdge) * tang;
        Vector2 pos = PointOnEdge(entryEdge, along, w, h) + inward * radius;
        return (pos, vel);
    }
}
