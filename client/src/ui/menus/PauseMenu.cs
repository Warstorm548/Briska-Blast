using System;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// In-match pause overlay (Esc) for an active multiplayer session — the menu in
/// the design mockup: the session code up top and three actions. Purely a view;
/// it surfaces the player's choice via events and lets the owning
/// <see cref="Game.GameScene"/> perform the leave / teardown, so the networking
/// lifecycle stays in one place. A <see cref="CanvasLayer"/> so it draws over the
/// Node2D match (and above the reconnect overlay) regardless of world transform.
/// </summary>
public partial class PauseMenu : CanvasLayer
{
    /// <summary>Resume the match (close the overlay).</summary>
    public event Action? ReturnRequested;
    /// <summary>Leave to the main menu. The leave is treated as a transient drop,
    /// so the slot is held for the reconnect grace window.</summary>
    public event Action? ExitToMenuRequested;
    /// <summary>Quit the application entirely (after signalling the drop).</summary>
    public event Action? QuitRequested;

    public override void _Ready()
    {
        GetNode<Label>("%CodeValue").Text = SessionContext.Instance?.SessionCode ?? "";

        GetNode<Button>("%ReturnButton").Pressed += () => ReturnRequested?.Invoke();
        GetNode<Button>("%ExitButton").Pressed += () => ExitToMenuRequested?.Invoke();
        GetNode<Button>("%QuitButton").Pressed += () => QuitRequested?.Invoke();

        var flash = GetNode<Label>("%CopiedFlash");
        GetNode<TextureButton>("%CopyCodeButton").Pressed += () => CopyCode(flash);

        // Land focus on the safe default so a keyboard player can resume without
        // tabbing onto a leave action first.
        GetNode<Button>("%ReturnButton").GrabFocus();
    }

    /// <summary>Copy the session code to the OS clipboard and flash a brief confirmation.</summary>
    private void CopyCode(Label flash)
    {
        var code = SessionContext.Instance?.SessionCode;
        if (string.IsNullOrEmpty(code))
            return;
        DisplayServer.ClipboardSet(code);
        flash.Visible = true;
        GetTree().CreateTimer(1.2).Timeout += () =>
        {
            if (GodotObject.IsInstanceValid(flash))
                flash.Visible = false;
        };
    }
}
