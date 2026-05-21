using Godot;

namespace BriskaBlast.UI.Menus;

public partial class JoinMenu : Control
{
    private LineEdit _codeInput = null!;

    public override void _Ready()
    {
        _codeInput = GetNode<LineEdit>("%CodeInput");
        GetNode<Button>("%JoinButton").Pressed += OnJoinPressed;
        _codeInput.TextSubmitted += _ => OnJoinPressed();
        GetNode<Button>("%ReturnButton").Pressed += () =>
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }

    private void OnJoinPressed()
    {
        var code = _codeInput.Text.Trim().ToUpperInvariant();
        if (code.Length != 6)
        {
            GD.Print($"Join: invalid code length ({code.Length})");
            return;
        }
        GD.Print($"Join: would request session '{code}' from server");
    }
}
