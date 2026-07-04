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

    // Live nodes built during a rejoin-into-match attempt (process-death
    // recovery). Handed to SessionContext on success, torn down on failure.
    private SignalingClient? _rejoinSignaling;
    private WebRtcMeshTransport? _rejoinTransport;

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
            SessionContext.Instance.StartJoinSession(code, r.Gamemode, r.PlayerCount, r.WinCondition, r.SpawnSettings, roster);
            GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
            return;
        }

        // The session is already live. If we're still a member (we dropped and
        // are rejoining within our window), the WS will let us back in; if not,
        // it rejects us and we surface a friendly message. Either way this is the
        // process-death rejoin path, not a fresh join.
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
        // Need the gamemode + player count to seed SessionContext; the
        // authoritative roster/host come from the WS Identified frame.
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

        var ctx = SessionContext.Instance;
        ctx.StartRejoinSession(code, s.Gamemode, s.PlayerCount, s.WinCondition, s.SpawnSettings);

        _rejoinSignaling = new SignalingClient();
        AddChild(_rejoinSignaling);
        _rejoinTransport = new WebRtcMeshTransport();
        _rejoinTransport.Init(_rejoinSignaling);
        AddChild(_rejoinTransport);

        _rejoinSignaling.Identified += OnRejoinIdentified;
        _rejoinSignaling.Closed += OnRejoinClosed;
        _rejoinSignaling.Connect(code, ctx.PlayerId, ctx.SecretToken);
    }

    /// <summary>Handle the rejoin <c>Identified</c> frame (process-death recovery):
    /// restore host, roster, and the frozen seating order, re-establish the WebRTC
    /// mesh, hand the live net to <see cref="SessionContext"/>, and enter the game
    /// scene.</summary>
    private void OnRejoinIdentified(string hostId, string[] peers, string[] seatOrder,
        bool isHost, System.Collections.Generic.Dictionary<string, string> usernames,
        IceServerDto[] iceServers)
    {
        // A rejoiner is a fresh process that missed the TURN credentials in
        // start_signaling; the server minted this identify its own set. Apply
        // before Connect below — safe ordering, peers only start offering to us
        // after this same Identify triggers their PeerJoined.
        _rejoinTransport!.SetIceServers(iceServers);
        var ctx = SessionContext.Instance;
        ctx.MergeUsernames(usernames);
        ctx.HostPlayerId = hostId;
        // Reproduce the exact seating the match froze at Start. The server's
        // Identified frame carries the frozen, self-inclusive seat_order (the live
        // session may have re-ordered after a host promotion while we were gone),
        // so our portals line up with everyone still in the match. See BuildEdges.
        ctx.SetSeatOrder(seatOrder);
        ctx.PlayerIds.Clear();
        if (!string.IsNullOrEmpty(hostId))
            ctx.PlayerIds.Add(hostId);
        if (ctx.PlayerId != hostId)
            ctx.PlayerIds.Add(ctx.PlayerId);
        foreach (var p in peers)
            if (p != hostId && p != ctx.PlayerId && !ctx.PlayerIds.Contains(p))
                ctx.PlayerIds.Add(p);

        // Re-establish the mesh to every current peer, then hand the live net to
        // SessionContext (survives the scene change) and enter the game.
        _rejoinTransport!.Connect(ctx.PlayerId, peers);

        _rejoinSignaling!.Identified -= OnRejoinIdentified;
        _rejoinSignaling.Closed -= OnRejoinClosed;
        ctx.AdoptNet(_rejoinSignaling, _rejoinTransport);
        _rejoinSignaling = null;
        _rejoinTransport = null;

        if (GetTree().ChangeSceneToFile("res://src/game/GameScene.tscn") != Error.Ok)
        {
            GD.PushError("[join] failed to enter GameScene on rejoin — returning to menu.");
            ctx.TeardownNet();
            ctx.ClearSession();
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
        }
    }

    private void OnRejoinClosed(int closeCode, string reason)
    {
        TeardownRejoinAttempt();
        SessionContext.Instance.ClearSession();
        SetBusy(false, closeCode switch
        {
            4403 => "You're not part of that match.",
            4404 => "That match no longer exists.",
            _ => "Could not rejoin the match.",
        });
    }

    private void TeardownRejoinAttempt()
    {
        if (_rejoinSignaling != null)
        {
            _rejoinSignaling.Identified -= OnRejoinIdentified;
            _rejoinSignaling.Closed -= OnRejoinClosed;
            _rejoinSignaling.CloseConnection();
            _rejoinSignaling.QueueFree();
            _rejoinSignaling = null;
        }
        if (_rejoinTransport != null)
        {
            _rejoinTransport.Close();
            _rejoinTransport.QueueFree();
            _rejoinTransport = null;
        }
    }

    private void SetBusy(bool busy, string message)
    {
        _joinButton.Disabled = busy;
        _status.Text = message;
        _status.Visible = !string.IsNullOrEmpty(message);
    }
}
