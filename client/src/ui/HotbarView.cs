using BriskaBlast.Core;
using BriskaBlast.Game;
using Godot;

namespace BriskaBlast.UI;

/// <summary>
/// The action bar pinned below the play field: a full-width strip holding a centered
/// row of flush item slots, each fired by its own number key.
///
/// Sizes are fractions of VIEWPORT height (not arena height) because this bar is what
/// defines the arena — <c>GameScene</c> subtracts <see cref="HeightHFrac"/> from the
/// viewport to get the play field, so the two can't both derive from each other. Every
/// dimension resolves from the runtime viewport rather than a hardcoded pixel count:
/// the logical viewport is only 2560×1440 on a 16:9 window and grows on one axis
/// otherwise (project.godot uses stretch aspect "expand"), so a fraction is the only
/// thing that means the same on every display.
/// </summary>
public partial class HotbarView : CanvasLayer
{
    /// <summary>One slot's edge length as a fraction of viewport height — 105px at the
    /// 2560×1440 design size, matching the slot sprite's native resolution.</summary>
    public const float SlotSizeHFrac = 105f / 1440f;

    /// <summary>Height of the whole strip. The bar is exactly one slot tall: the slots
    /// sit flush against its top and bottom, so the light-gray backing shows only to the
    /// left and right of the row.</summary>
    public const float HeightHFrac = SlotSizeHFrac;

    // The slot sprite is a 105×105 frame: 6px of solid black on all four sides around a
    // 93×93 inner square that fades to a teal center. An item icon fills that inner
    // square exactly. Kept as ratios of the sprite so both survive the bar being scaled
    // to a viewport and the placeholder art being redrawn at a different resolution.
    // Note the frame is capped at 6px rather than scaled with the slot, so the icon area
    // grew 2.58× when the slot grew 2.5× — the ratios below are not simply the old ones.
    private const float IconInsetFrac = 6f / 105f;
    private const float IconSizeFrac = 93f / 105f;

    /// <summary>Placeholder backing colour. Deliberately plain — the slot art is a
    /// first pass and this bar is expected to be restyled with it.</summary>
    private static readonly Color StripColor = new(0.78f, 0.78f, 0.78f);

    // A fired slot flashes white and fades out. Short enough to read as a keypress
    // acknowledgement rather than an animation.
    private const float FlashPeakAlpha = 0.45f;
    private const ulong FlashMsec = 120;

    private readonly TextureRect[] _icons = new TextureRect[Hotbar.SlotCount];
    private readonly ColorRect[] _flashes = new ColorRect[Hotbar.SlotCount];

    /// <summary>Wall-clock deadline per slot; 0 means "not flashing".</summary>
    private readonly ulong[] _flashUntilMsec = new ulong[Hotbar.SlotCount];

    /// <summary>How many slots are mid-flash, so an idle bar costs one comparison a
    /// frame instead of touching every node.</summary>
    private int _activeFlashes;

    public override void _Ready()
    {
        float slot = SlotSizeHFrac * GetViewportSize().Y;
        float inset = slot * IconInsetFrac;

        var strip = new ColorRect { Color = StripColor };
        strip.SetAnchorsPreset(Control.LayoutPreset.BottomWide);
        strip.OffsetLeft = 0;
        strip.OffsetRight = 0;
        strip.OffsetTop = -(HeightHFrac * GetViewportSize().Y);
        strip.OffsetBottom = 0;
        strip.MouseFilter = Control.MouseFilterEnum.Ignore;
        AddChild(strip);

        // CenterContainer keeps the row centered in a full-width strip, which is what
        // leaves the backing visible at both ends of the screen.
        var center = new CenterContainer();
        center.SetAnchorsPreset(Control.LayoutPreset.FullRect);
        center.MouseFilter = Control.MouseFilterEnum.Ignore;
        strip.AddChild(center);

        // Zero separation is what makes the slots share edges with no seam between them.
        var row = new HBoxContainer();
        row.AddThemeConstantOverride("separation", 0);
        row.MouseFilter = Control.MouseFilterEnum.Ignore;
        center.AddChild(row);

        var frame = SpriteRegistry.Instance.GetTexture(AssetId.ItemSlot);

        for (int i = 0; i < Hotbar.SlotCount; i++)
        {
            var cell = new TextureRect
            {
                Texture = frame,
                CustomMinimumSize = new Vector2(slot, slot),
                StretchMode = TextureRect.StretchModeEnum.Scale,
                ExpandMode = TextureRect.ExpandModeEnum.IgnoreSize,
                MouseFilter = Control.MouseFilterEnum.Ignore,
            };
            row.AddChild(cell);

            // Inset on every side by the frame thickness, so the icon lands in the
            // sprite's inner square and never paints over the black border.
            var icon = new TextureRect
            {
                Visible = false,
                StretchMode = TextureRect.StretchModeEnum.Scale,
                ExpandMode = TextureRect.ExpandModeEnum.IgnoreSize,
                MouseFilter = Control.MouseFilterEnum.Ignore,
            };
            icon.SetAnchorsPreset(Control.LayoutPreset.FullRect);
            icon.OffsetLeft = inset;
            icon.OffsetTop = inset;
            icon.OffsetRight = -inset;
            icon.OffsetBottom = -inset;
            cell.AddChild(icon);
            _icons[i] = icon;

            // Covers the whole cell, frame included — a keypress should light up the
            // slot, not just its contents.
            var flash = new ColorRect
            {
                Color = new Color(1, 1, 1, 0),
                MouseFilter = Control.MouseFilterEnum.Ignore,
            };
            flash.SetAnchorsPreset(Control.LayoutPreset.FullRect);
            cell.AddChild(flash);
            _flashes[i] = flash;
        }
    }

    public override void _Process(double delta)
    {
        if (_activeFlashes == 0)
            return;

        ulong now = Time.GetTicksMsec();
        _activeFlashes = 0;

        for (int i = 0; i < Hotbar.SlotCount; i++)
        {
            ulong until = _flashUntilMsec[i];
            if (until == 0)
                continue;

            if (now >= until)
            {
                _flashUntilMsec[i] = 0;
                _flashes[i].Color = new Color(1, 1, 1, 0);
                continue;
            }

            // Fade the remaining lifetime out to nothing.
            float t = (until - now) / (float)FlashMsec;
            _flashes[i].Color = new Color(1, 1, 1, FlashPeakAlpha * t);
            _activeFlashes++;
        }
    }

    /// <summary>Light a slot up briefly to acknowledge its key. Purely cosmetic — the
    /// bar does not know or care whether the slot held anything.</summary>
    public void Flash(int index)
    {
        if (index < 0 || index >= Hotbar.SlotCount)
            return;

        _flashUntilMsec[index] = Time.GetTicksMsec() + FlashMsec;
        _flashes[index].Color = new Color(1, 1, 1, FlashPeakAlpha);
        _activeFlashes++;
    }

    /// <summary>Repaint the row from the model: show each slot's icon, or hide it when
    /// the slot is empty. The single hook a future item system calls after changing what
    /// the player holds.</summary>
    public void SyncFrom(Hotbar hotbar)
    {
        for (int i = 0; i < Hotbar.SlotCount; i++)
        {
            var slot = hotbar.Slots[i];
            var icon = _icons[i];

            if (slot.IsEmpty)
            {
                icon.Visible = false;
                icon.Texture = null;
                continue;
            }

            icon.Texture = SpriteRegistry.Instance.GetTexture(slot.Icon!.Value);
            icon.Visible = true;
        }
    }

    private Vector2 GetViewportSize() => GetViewport().GetVisibleRect().Size;
}
