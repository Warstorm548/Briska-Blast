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
            SessionContext.Instance.StartJoinSession(code, r.Gamemode, r.PlayerCount, r.WinCondition, r.SpawnSettings,
                r.LootSettings, roster);
            GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
            return;
        }

        // The session is already live. If we're still a member (we dropped and
        // are rejoining within our window), the WS will let us back in; if not,
        // it rejects us and MatchFlow surfaces a friendly message on the main
        // menu. Either way this is the process-death rejoin path, not a fresh join.
        if (result.ErrorCode == "session_already_active")
        {
            BeginRejoin(code);
            return;
        }

        // Map the server's error codes to friendlier text.
        var message = result.ErrorCode switch
        {
            "not_found" => "No session with that code.",
            "session_full" => "That session is full.",
            "cannot_join_own_session" => "You're already hosting that session.",
            "already_joined" => "You've already joined that session.",
            _ => $"Could not join: {result.ErrorCode}",
        };
        SetBusy(false, message);
    }

    // ---- rejoin a live match (process-death recovery) ----

    private async void BeginRejoin(string code)
    {
        SetBusy(true, "Rejoining match…");
        // Need the gamemode + rules to seed SessionContext; the authoritative
        // roster/host/seating come from the WS Identified frame, which
        // MatchFlow consumes on its way into Preparing.
        var info = await SessionContext.Instance.Api.GetSessionAsync(code);
        Callable.From(() => OnRejoinInfo(info, code)).CallDeferred();
    }

    private void OnRejoinInfo(ApiResult<SessionPollResponse> info, string code)
    {
        if (!info.Ok || info.Value is not { } s)
        {
            SetBusy(false, info.ErrorCode == "not_found"
                ? "That match no longer exists."
                : "Could not rejoin the match.");
            return;
        }

        SessionContext.Instance.StartRejoinSession(code, s.Gamemode, s.PlayerCount, s.WinCondition, s.SpawnSettings,
            s.LootSettings);
        // MatchFlow owns the rest: fresh signaling socket, seat restore from the
        // Identified frame, mesh bring-up behind the connecting screen, and the
        // failure path (rejection lands on the main menu with the reason).
        MatchFlow.Instance.BeginRejoin();
    }

    private void SetBusy(bool busy, string message)
    {
        _joinButton.Disabled = busy;
        _status.Text = message;
        _status.Visible = !string.IsNullOrEmpty(message);
    }
}
