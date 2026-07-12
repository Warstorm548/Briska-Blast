using Godot;

namespace BriskaBlast.UI.Menus;

public partial class CreditsMenu : Control
{
    public override void _Ready()
    {
        GetNode<Button>("%ReturnButton").Pressed += () =>
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }
}
