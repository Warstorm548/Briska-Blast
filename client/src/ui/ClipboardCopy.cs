using Godot;

namespace BriskaBlast.UI;

/// <summary>Shared "copy to the OS clipboard, then briefly flash a confirmation label"
/// behavior for the session-code copy buttons in the lobby and the pause menu.</summary>
public static class ClipboardCopy
{
    /// <summary>Copy <paramref name="text"/> to the OS clipboard (no-op when empty), reveal
    /// <paramref name="flash"/>, then hide it again after <paramref name="seconds"/>. The
    /// timed hide guards <see cref="GodotObject.IsInstanceValid"/> in case the menu closed
    /// in the meantime.</summary>
    public static void CopyWithFlash(SceneTree tree, string? text, Label flash, double seconds = 1.2)
    {
        if (string.IsNullOrEmpty(text))
            return;
        DisplayServer.ClipboardSet(text);
        flash.Visible = true;
        tree.CreateTimer(seconds).Timeout += () =>
        {
            if (GodotObject.IsInstanceValid(flash))
                flash.Visible = false;
        };
    }
}
