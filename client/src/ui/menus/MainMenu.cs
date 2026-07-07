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
            status.SetAnchorsPreset(LayoutPreset.BottomWide);
            status.Position = new Vector2(status.Position.X, status.Position.Y - 60);
            status.AddThemeFontSizeOverride("font_size", 28);
            AddChild(status);
        }
    }

    private void GoTo(string scenePath)
    {
        GetTree().ChangeSceneToFile(scenePath);
    }
}
