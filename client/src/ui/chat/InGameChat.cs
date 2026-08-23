using System;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game;

namespace BriskaBlast.UI.Chat;

/// <summary>
/// The in-match chat window: a <see cref="ChatPanel"/> pinned to the bottom-left,
/// in the strip the action bar already occupies.
///
/// It is always visible — the keys only move keyboard focus, never visibility —
/// and it rests exactly one action-bar strip tall so the play field is untouched.
/// Focusing it grows the log UPWARD over the bottom-left of the field, which is
/// the only way to show a readable amount of history without shrinking the arena.
/// Arena height is derived from the strip (<see cref="GameScene"/>), and every
/// client in a match must agree on it, so growing the strip would be a protocol
/// change in all but name. Growing over it costs nothing.
///
/// Because the expanded panel covers live play, its background is translucent:
/// the ball, the paddle and the barriers stay readable through it. Typing during
/// a live match is the player's own risk to take — the simulation never pauses
/// for chat — so the window must never hide what it is costing them.
/// </summary>
public partial class InGameChat : CanvasLayer
{
    /// <summary>
    /// Height while focused, as a fraction of viewport height.
    ///
    /// Sized from the panel's own metrics rather than picked: compact chrome
    /// (margins, borders, separation and the input) measures 55px at the design
    /// size and a log line is 23px, so 10 lines of history needs
    /// 55 + 10×23 = 285, rounded to 288. Re-derive this if the theme's font size
    /// or the compact padding changes — the constant is arithmetic, not taste.
    ///
    /// The same height-relative convention the action bar uses: the logical
    /// viewport is only 2560×1440 at 16:9 and grows on one axis otherwise, so a
    /// fraction is the only thing that means the same on every display.
    /// </summary>
    private const float ExpandedHFrac = 288f / 1440f;

    /// <summary>Gap between the panel's right edge and the action bar's slot row,
    /// so the two never touch.</summary>
    private const float GutterHFrac = 16f / 1440f;

    /// <summary>Background alpha while the panel overlaps the play field. The
    /// border and every glyph stay fully opaque — only the fill is see-through,
    /// so text reads against a moving field instead of dissolving into it.</summary>
    private const float BackgroundAlpha = 0.6f;

    /// <summary>The input took or lost keyboard focus. <see cref="GameScene"/>
    /// suspends player controls on true and restores them on false.</summary>
    public event Action<bool>? FocusChanged;

    private ChatPanel _panel = null!;

    public override void _Ready()
    {
        _panel = GD.Load<PackedScene>("res://src/ui/chat/ChatPanel.tscn").Instantiate<ChatPanel>();
        AddChild(_panel);

        // No caption in-match: the resting strip is one action-bar slot tall and
        // cannot afford a line that only says "Chat". Tight padding for the same
        // reason — the input already claims about half the resting height.
        _panel.ShowHeader(false);
        _panel.SetCompact(true);
        _panel.AddThemeStyleboxOverride("panel", TranslucentPanel());

        // Pinned bottom-left. Only the top offset moves when the panel grows, so
        // it expands upward into the field and never shifts off the strip.
        _panel.AnchorLeft = 0;
        _panel.AnchorRight = 0;
        _panel.AnchorTop = 1;
        _panel.AnchorBottom = 1;
        _panel.OffsetLeft = 0;
        _panel.OffsetBottom = 0;
        _panel.OffsetRight = PanelWidth();
        ApplyHeight(expanded: false);

        _panel.InputFocusChanged += OnFocusChanged;
    }

    public override void _ExitTree()
    {
        if (_panel != null)
            _panel.InputFocusChanged -= OnFocusChanged;
    }

    /// <summary>Render and follow the session transcript carried in from the lobby.</summary>
    public void Bind(ChatLog log) => _panel.Bind(log);

    /// <summary>Drop keyboard focus (Escape, or the match ending).</summary>
    public void ReleaseInput() => _panel.ReleaseInput();

    public override void _UnhandledInput(InputEvent @event)
    {
        // Opening is handled as an EVENT, unlike every other input in the match,
        // which is polled. A focused LineEdit consumes key events before they
        // reach unhandled input, so this cannot fire while the player is typing —
        // which is exactly what lets "t" be a letter rather than a second toggle.
        // Polling Input.IsActionJustPressed here would re-trigger on every "t".
        if (@event.IsActionPressed("chat_command"))
        {
            // Command style: the slash is left in the field so a future dev-tools
            // parser has its prefix already typed.
            _panel.FocusInput("/");
            GetViewport().SetInputAsHandled();
        }
        else if (@event.IsActionPressed("chat_focus"))
        {
            _panel.FocusInput();
            GetViewport().SetInputAsHandled();
        }
    }

    private void OnFocusChanged(bool focused)
    {
        ApplyHeight(focused);
        FocusChanged?.Invoke(focused);
    }

    // Resting height is the action-bar strip exactly, so an unfocused panel sits
    // entirely below the play field and covers nothing. The compact chrome fits
    // inside it with room for a line or two of history — enough to notice someone
    // talking, which is all the resting state has to do.
    private void ApplyHeight(bool expanded)
    {
        var vp = GetViewportSize();
        float h = expanded
            ? ExpandedHFrac * vp.Y
            : HotbarView.HeightHFrac * vp.Y;
        _panel.OffsetTop = -h;
    }

    // The action bar centres its slot row in a full-width strip, leaving the ends
    // bare. The panel claims the left one — derived from the row, never assumed,
    // so it still lands correctly when the viewport is not 16:9.
    private float PanelWidth()
    {
        var vp = GetViewportSize();
        float slot = HotbarView.SlotSizeHFrac * vp.Y;
        float rowWidth = Hotbar.SlotCount * slot;
        return (vp.X - rowWidth) * 0.5f - GutterHFrac * vp.Y;
    }

    // Mirrors MenuTheme's InnerPanel variation — same border and corner radius so
    // it reads as the same component as the lobby's — with the fill alpha dropped
    // from 0.9 to BackgroundAlpha.
    private static StyleBoxFlat TranslucentPanel()
    {
        var box = new StyleBoxFlat
        {
            BgColor = new Color(0.03f, 0.07f, 0.16f, BackgroundAlpha),
            BorderColor = new Color(0.2f, 0.4f, 0.7f, 1f),
            CornerRadiusTopLeft = 8,
            CornerRadiusTopRight = 8,
            CornerRadiusBottomRight = 8,
            CornerRadiusBottomLeft = 8,
        };
        box.SetBorderWidthAll(2);
        return box;
    }

    private Vector2 GetViewportSize() => GetViewport().GetVisibleRect().Size;
}
