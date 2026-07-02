using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// Static geometry for the four corner barriers — a single source of truth shared by
/// the simulation (which reads the collision rects) and the view (which places the
/// sprites), so the collider can never drift from the art.
///
/// The <c>Cornerbarrier</c> sprite is drawn as an L for the BOTTOM-LEFT corner. The
/// other three corners are the SAME sprite rotated in 90° steps about its very
/// bottom-left pixel, with that pixel pinned to the screen corner. Every L opens
/// toward the arena centre. The barrier is sized as a fraction of arena HEIGHT (like
/// the paddle / ball / goal-gap tuning) so every resolution gets the same proportion.
/// </summary>
public static class CornerBarrier
{
    /// <summary>Native sprite size the local footprint below was measured against.</summary>
    public const float TexW = 100f;
    public const float TexH = 120f;

    /// <summary>Barrier height as a fraction of arena height (renders at native design
    /// size on a 1440-tall arena). The single knob for the barrier's overall size.</summary>
    public const float HeightHFrac = 120f / 1440f;

    // Opaque L footprint measured from the PNG alpha, in sprite-local pixels (texture
    // space: origin top-left, y DOWN). The transparent inner region is left open, so a
    // ball can occupy the concave notch — the collider is just the two solid bars.
    private const float ArmW = 25f;   // vertical bar: x[0,25]  y[0,120]
    private const float FootH = 20f;  // horizontal bar: x[0,100] y[100,120]

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

    /// <summary>Append the 8 axis-aligned collision rects (arm + foot per corner) in
    /// arena-pixel coordinates. Derived from the SAME transform the view uses to place
    /// each sprite: <c>world = pivot + Rot(θ)·scale·(local − bottomLeftPixel)</c>. Because
    /// θ is a multiple of 90° every rect stays axis-aligned.</summary>
    public static void AppendRects(List<Rect2> into, float w, float h)
    {
        float s = ScaleFor(h);
        var blPixel = new Vector2(0f, TexH); // bottom-left pixel in texture space

        // The two solid bars of the L, as texture-space rects.
        var arm = new Rect2(0f, 0f, ArmW, TexH);
        var foot = new Rect2(0f, TexH - FootH, TexW, FootH);

        foreach (var (corner, rot) in Corners)
        {
            var xf = new Transform2D(rot, Pivot(corner, w, h));
            into.Add(ToArena(arm, blPixel, s, xf));
            into.Add(ToArena(foot, blPixel, s, xf));
        }
    }

    // Map a texture-space rect to an arena AABB. For 90° multiples the image is
    // axis-aligned, so transforming two opposite corners and taking min/max is exact.
    private static Rect2 ToArena(Rect2 local, Vector2 blPixel, float s, Transform2D xf)
    {
        var a = xf * ((local.Position - blPixel) * s);
        var b = xf * ((local.End - blPixel) * s);
        var min = new Vector2(Mathf.Min(a.X, b.X), Mathf.Min(a.Y, b.Y));
        var max = new Vector2(Mathf.Max(a.X, b.X), Mathf.Max(a.Y, b.Y));
        return new Rect2(min, max - min);
    }
}
