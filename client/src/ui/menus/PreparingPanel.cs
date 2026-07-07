using Godot;
using System;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// Reusable "connecting…" panel: a title, a status line, the session code with
/// a copy button, and an optional Cancel. Used full-screen by
/// <see cref="PreparingScreen"/> while the WebRTC mesh comes up, and reused as
/// an in-game overlay for the pause-on-rejoin flow. A pure view: it raises
/// <see cref="CancelPressed"/> and never touches the lifecycle itself.
/// </summary>
public partial class PreparingPanel : PanelContainer
{
    /// <summary>The player pressed Cancel. The owner decides what that means.</summary>
    public event Action? CancelPressed;

    public override void _Ready()
    {
        GetNode<Label>("%CodeValue").Text = SessionContext.Instance?.SessionCode ?? "";
        GetNode<Button>("%CancelButton").Pressed += () => CancelPressed?.Invoke();
        GetNode<TextureButton>("%CopyCodeButton").Pressed += () =>
            ClipboardCopy.CopyWithFlash(GetTree(), SessionContext.Instance?.SessionCode,
                GetNode<Label>("%CopiedFlash"));
    }

    /// <summary>Set the headline (e.g. "Connecting to players…").</summary>
    public void SetTitle(string title) => GetNode<Label>("%TitleLabel").Text = title;

    /// <summary>Set the progress/status line (e.g. "Connected 1/3…").</summary>
    public void SetStatus(string status) => GetNode<Label>("%StatusLabel").Text = status;

    /// <summary>Show or hide the Cancel button (the pause overlay hides it —
    /// mid-match there is nothing local to cancel).</summary>
    public void ShowCancel(bool visible) => GetNode<Button>("%CancelButton").Visible = visible;
}
