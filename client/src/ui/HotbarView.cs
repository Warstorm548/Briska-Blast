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
    /// <summary>One slot's edge length as a fraction of viewport height — 96px at the
    /// 2560×1440 design size, matching the slot sprite's native resolution.</summary>
    public const float SlotSizeHFrac = 96f / 1440f;

    /// <summary>Height of the whole strip. The bar is exactly one slot tall: the slots
    /// sit flush against its top and bottom, so the metallic backing shows only to the
    /// left and right of the row.</summary>
    public const float HeightHFrac = SlotSizeHFrac;

    // The slot sprite is a 96×96 frame: 6px of near-black (#191919) on all four sides
    // around an 84×84 inner square that fades to a teal center. An item icon fills that
    // inner square exactly. Kept as a ratio of the sprite so it survives the bar being
    // scaled to a viewport and the art being redrawn at a different resolution. The
    // border is subtracted from BOTH sides, so the interior is 96 − 2×6, not 96 − 6.
    private const float IconInsetFrac = 6f / 96f;

    /// <summary>Backing colour behind the slots: a flat, slightly blue-tinted mid-dark
    /// gray. The cool tint is what reads as brushed metal rather than flat gray, and it
    /// picks up the teal in the slot art.</summary>
    private static readonly Color StripColor = new(0.42f, 0.44f, 0.47f);

    // A fired slot flashes white and fades out. Short enough to read as a keypress
    // acknowledgement rather than an animation.
    private const float FlashPeakAlpha = 0.45f;
    private const ulong FlashMsec = 120;

    private readonly TextureRect[] _icons = new TextureRect[Hotbar.SlotCount];
    private readonly ColorRect[] _flashes = new ColorRect[Hotbar.SlotCount];

    /// <summary>Per-slot stack count, pinned to the slot's top-right corner.</summary>
    private readonly Label[] _counts = new Label[Hotbar.SlotCount];

    /// <summary>Active-effect readout, sitting to the RIGHT of the slot row rather
    /// than inside any slot. That placement is load-bearing, not decorative: spending
    /// an item's last charge clears its slot, and the effect it bought is still
    /// running — a countdown drawn inside the slot would vanish exactly when the
    /// player most needs to see it. One row per live effect, so a second item's timer
    /// stacks underneath for free.</summary>
    private VBoxContainer _effects = null!;
    private Label _shieldEffect = null!;

    /// <summary>Font size for the slot count, as a fraction of the slot's edge. The
    /// bar scales with the viewport, so a fixed pt size would be wrong everywhere but
    /// the design resolution.</summary>
    private const float CountFontFrac = 26f / 96f;
    private const float EffectFontFrac = 30f / 96f;

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

            // Stack count, top-right. Inset by the SAME frame thickness the icon
            // uses, so it sits inside the teal interior and never paints over the
            // 6px border. Added after the flash so a keypress still lights the whole
            // slot including the number.
            var count = new Label
            {
                Visible = false,
                HorizontalAlignment = HorizontalAlignment.Right,
                VerticalAlignment = VerticalAlignment.Top,
                MouseFilter = Control.MouseFilterEnum.Ignore,
            };
            count.SetAnchorsPreset(Control.LayoutPreset.FullRect);
            count.OffsetLeft = inset;
            count.OffsetTop = inset;
            count.OffsetRight = -inset;
            count.OffsetBottom = -inset;
            count.AddThemeFontSizeOverride("font_size", Mathf.RoundToInt(slot * CountFontFrac));
            count.AddThemeColorOverride("font_color", new Color(1f, 1f, 1f));
            // The interior is a mid-tone teal, so an unoutlined glyph loses contrast
            // against the lighter centre of the slot art.
            count.AddThemeColorOverride("font_outline_color", new Color(0f, 0f, 0f, 0.85f));
            count.AddThemeConstantOverride("outline_size", Mathf.Max(1, Mathf.RoundToInt(slot * 0.03f)));
            cell.AddChild(count);
            _counts[i] = count;
        }

        // Active-effect readout, right-aligned in the metallic backing beside the
        // row. The row is centred, so at the design size this gutter is ~1040px —
        // ample space, and it is otherwise empty.
        _effects = new VBoxContainer
        {
            Alignment = BoxContainer.AlignmentMode.Center,
            MouseFilter = Control.MouseFilterEnum.Ignore,
        };
        _effects.SetAnchorsPreset(Control.LayoutPreset.RightWide);
        _effects.OffsetLeft = -(slot * 5f);
        _effects.OffsetRight = -inset * 2f;
        strip.AddChild(_effects);

        _shieldEffect = new Label
        {
            Visible = false,
            HorizontalAlignment = HorizontalAlignment.Right,
            MouseFilter = Control.MouseFilterEnum.Ignore,
        };
        _shieldEffect.AddThemeFontSizeOverride("font_size", Mathf.RoundToInt(slot * EffectFontFrac));
        _shieldEffect.AddThemeColorOverride("font_color", new Color(0.62f, 0.92f, 1f));
        _shieldEffect.AddThemeColorOverride("font_outline_color", new Color(0f, 0f, 0f, 0.85f));
        _shieldEffect.AddThemeConstantOverride("outline_size", Mathf.Max(1, Mathf.RoundToInt(slot * 0.03f)));
        _effects.AddChild(_shieldEffect);
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
                // A spent slot is cleared, not shown holding zero — so the count
                // goes with the icon and the slot reads as free for any item.
                _counts[i].Visible = false;
                continue;
            }

            icon.Texture = SpriteRegistry.Instance.GetTexture(slot.Icon!.Value);
            icon.Visible = true;

            // A lone item doesn't need a "1" cluttering the slot; the icon says it.
            _counts[i].Text = slot.Count.ToString();
            _counts[i].Visible = slot.Count > 1;
        }
    }

    /// <summary>Refresh the active-effect readout beside the row. Driven from the
    /// game state's effect timers rather than from slot contents — which is exactly
    /// what keeps a running barrier on screen after its last charge was spent and its
    /// slot cleared. Called every frame, like the play-field render.</summary>
    public void SyncEffects(GameState state)
    {
        if (state.ShieldActive)
        {
            // Ceiling, so a barrier with 0.3s left still reads "1s" rather than
            // showing 0 while it is demonstrably still blocking balls.
            int secs = Mathf.CeilToInt(state.ShieldSecsRemaining);
            _shieldEffect.Text = $"{ItemRegistry.DisplayName(ItemId.BarrierShield)}  {secs}s";
            _shieldEffect.Visible = true;
        }
        else
        {
            _shieldEffect.Visible = false;
        }
    }

    private Vector2 GetViewportSize() => GetViewport().GetVisibleRect().Size;
}
