using Godot;
using System.Linq;
using BriskaBlast.Core;
using BriskaBlast.Net;

namespace BriskaBlast.UI.Menus;

public partial class JoinMenu : Control
{
    private LineEdit _codeInput = null!;
    private Button _joinButton = null!;
    private Label _status = null!;

    public override void _Ready()
    {
        _codeInput = GetNode<LineEdit>("%CodeInput");
        _joinButton = GetNode<Button>("%JoinButton");

        _status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
        _codeInput.GetParent().AddChild(_status);

        _joinButton.Pressed += OnJoinPressed;
        _codeInput.TextSubmitted += _ => OnJoinPressed();
        GetNode<Button>("%ReturnButton").Pressed += () =>
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }

    private async void OnJoinPressed()
    {
        var code = _codeInput.Text.Trim().ToUpperInvariant();
        if (code.Length != 6)
        {
            SetBusy(false, "Session codes are 6 characters.");
            return;
        }

        SetBusy(true, "Joining…");

        var ctx = SessionContext.Instance;
        if (!await ctx.EnsureIdentityAsync())
        {
            Callable.From(() => SetBusy(false, "No identity — launch via the launcher.")).CallDeferred();
            return;
        }

        var result = await ctx.Api.JoinAsync(code, ctx.PlayerId, ctx.SecretToken);
        Callable.From(() => OnJoinComplete(result, code)).CallDeferred();
    }

    private void OnJoinComplete(ApiResult<JoinResponse> result, string code)
    {
        if (result.Ok && result.Value is { } r)
        {
            var roster = r.Joiners.Select(j => j.PlayerId);
            SessionContext.Instance.StartJoinSession(code, r.Gamemode, r.PlayerCount, roster);
            GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
            return;
        }

        // Map the server's error codes to friendlier text.
        var message = result.ErrorCode switch
        {
            "not_found" => "No session with that code.",
            "session_full" => "That session is full.",
            "session_already_active" => "That session has already started.",
            "cannot_join_own_session" => "You're already hosting that session.",
            "already_joined" => "You've already joined that session.",
            _ => $"Could not join: {result.ErrorCode}",
        };
        SetBusy(false, message);
    }

    private void SetBusy(bool busy, string message)
    {
        _joinButton.Disabled = busy;
        _status.Text = message;
        _status.Visible = !string.IsNullOrEmpty(message);
    }
}
