using Godot;

namespace BriskaBlast.UI.Menus;

public partial class MainMenu : Control
{
    public override void _Ready()
    {
        // Belt-and-braces with SingleInstance: if this process is a duplicate
        // game (the autoload already queued a deferred quit), bail before wiring
        // anything up so the main scene never meaningfully runs.
        if (BriskaBlast.Core.SingleInstance.IsDuplicate)
        {
            GetTree().Quit();
            return;
        }

        GetNode<Button>("%HostGame").Pressed += () => GoTo("res://src/ui/menus/HostSetupMenu.tscn");
        GetNode<Button>("%JoinGame").Pressed += () => GoTo("res://src/ui/menus/JoinMenu.tscn");
        GetNode<Button>("%Settings").Pressed += () => GoTo("res://src/ui/menus/SettingsMenu.tscn");
        GetNode<Button>("%ExitGame").Pressed += () => GetTree().Quit();

        // If a session just ended abnormally (connect timeout, kick, session
        // closed, rejoin refused), MatchFlow lands the player here — tell them
        // why. Read-and-clear so the message shows once.
        var flowError = BriskaBlast.Core.MatchFlow.Instance?.TakeFlowError();
        if (!string.IsNullOrEmpty(flowError))
        {
            var status = new Label
            {
                Text = flowError,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            // Anchor to the bottom edge and lift it via offsets (not Position,
            // which wouldn't track a window resize).
            status.SetAnchorsPreset(LayoutPreset.BottomWide);
            status.OffsetTop = -100;
            status.OffsetBottom = -60;
            status.AddThemeFontSizeOverride("font_size", 28);
            // Warning tint so an abnormal session end reads as one at a glance.
            status.AddThemeColorOverride("font_color", new Color(1f, 0.45f, 0.45f));
            AddChild(status);
        }
    }

    private void GoTo(string scenePath)
    {
        GetTree().ChangeSceneToFile(scenePath);
    }
}
