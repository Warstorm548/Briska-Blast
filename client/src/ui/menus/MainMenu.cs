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
    }

    private void GoTo(string scenePath)
    {
        GetTree().ChangeSceneToFile(scenePath);
    }
}
