using Godot;
using BriskaBlast.Core;
using BriskaBlast.Net;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// Live lobby. Opens a signaling WebSocket on entry, drives the roster from
/// server events (Identified / PeerJoined / PeerLeft / HostChanged), and
/// backs the buttons with the REST endpoints. Stage 1 stops at
/// <c>start_signaling</c> — WebRTC + gameplay are later stages.
/// </summary>
public partial class SessionLobby : Control
{
    private HBoxContainer[] _slots = null!;
    private SignalingClient _signaling = null!;
    private Label _status = null!;
    private bool _leaving;

    public override void _Ready()
    {
        _slots = new[]
        {
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot1"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot2"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot3"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot4"),
        };

        for (int i = 0; i < _slots.Length; i++)
        {
            var idx = i;
            _slots[i].GetNode<Button>("Promote").Pressed += () => OnPromote(idx);
        }

        GetNode<Button>("%StartSessionButton").Pressed += OnStartPressed;
        GetNode<Button>("%CancelSessionButton").Pressed += OnCancelOrLeavePressed;
        GetNode<Button>("%ReturnToSetupButton").Pressed += OnReturnToSetupPressed;

        _status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
        GetNode("LeftPanel/LeftMargins/LeftContent").AddChild(_status);

        // Connect signaling. The SignalingClient is a child so it polls on the
        // main thread — every callback below runs on the main thread and may
        // touch the tree directly.
        _signaling = new SignalingClient();
        AddChild(_signaling);
        _signaling.Identified += OnIdentified;
        _signaling.PeerJoined += OnPeerJoined;
        _signaling.PeerLeft += OnPeerLeft;
        _signaling.HostChanged += OnHostChanged;
        _signaling.StartSignaling += OnStartSignaling;
        _signaling.SessionEnded += reason => LeaveToMenu($"Session ended ({reason}).");
        _signaling.Kicked += reason => LeaveToMenu($"Removed from session ({reason}).");
        _signaling.Closed += OnClosed;

        var ctx = SessionContext.Instance;
        _signaling.Connect(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);

        Render();
    }

    // ---- signaling callbacks (main thread) ----

    private void OnIdentified(string hostId, string[] peers, bool isHost)
    {
        var ctx = SessionContext.Instance;
        ctx.HostPlayerId = hostId;
        ctx.PlayerIds.Clear();
        if (!string.IsNullOrEmpty(hostId))
            ctx.PlayerIds.Add(hostId);
        if (ctx.PlayerId != hostId)
            ctx.PlayerIds.Add(ctx.PlayerId);
        foreach (var p in peers)
            if (p != hostId && p != ctx.PlayerId && !ctx.PlayerIds.Contains(p))
                ctx.PlayerIds.Add(p);
        Render();
    }

    private void OnPeerJoined(string playerId)
    {
        var ids = SessionContext.Instance.PlayerIds;
        if (!ids.Contains(playerId))
            ids.Add(playerId);
        Render();
    }

    private void OnPeerLeft(string playerId, string reason)
    {
        SessionContext.Instance.PlayerIds.Remove(playerId);
        Render();
    }

    private void OnHostChanged(string playerId)
    {
        SessionContext.Instance.HostPlayerId = playerId;
        Render();
    }

    private void OnStartSignaling(string gamemode, int playerCount, string[] peers)
    {
        // Stage 1 endpoint: the lobby is fully wired up to here. WebRTC peer
        // negotiation and gameplay arrive in later stages.
        GetNode<Button>("%StartSessionButton").Disabled = true;
        ShowStatus($"All {playerCount} players ready — starting… (gameplay lands in a later stage)");
    }

    private void OnClosed(int code, string reason)
    {
        if (_leaving)
            return;
        // 1000 is a normal close; anything else (4xxx app codes, 1006 abnormal)
        // means we lost the lobby unexpectedly.
        LeaveToMenu(code == 1000 ? "Disconnected from session." : $"Connection closed ({code}).");
    }

    // ---- buttons ----

    private async void OnStartPressed()
    {
        var ctx = SessionContext.Instance;
        ShowStatus("Starting…");
        var result = await ctx.Api.StartSessionAsync(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
        if (!result.Ok)
        {
            var why = result.ErrorCode switch
            {
                "session_not_startable" => "Not everyone is connected yet.",
                _ => $"Could not start: {result.ErrorCode}",
            };
            Callable.From(() => ShowStatus(why)).CallDeferred();
        }
        // Success → the server broadcasts start_signaling; OnStartSignaling handles it.
    }

    private async void OnCancelOrLeavePressed()
    {
        var ctx = SessionContext.Instance;
        if (ctx.LocalPlayerIsHost)
        {
            // Host cancels → tear the session down server-side.
            await ctx.Api.CloseSessionAsync(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
            Callable.From(() => LeaveToMenu("")).CallDeferred();
        }
        else
        {
            // Joiner leaves → explicit Leave frees the slot in the lobby.
            _signaling.SendLeave();
            LeaveToMenu("");
        }
    }

    private async void OnReturnToSetupPressed()
    {
        var ctx = SessionContext.Instance;
        await ctx.Api.CloseSessionAsync(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
        Callable.From(() =>
        {
            _leaving = true;
            _signaling.CloseConnection();
            SessionContext.Instance.ClearSession();
            GetTree().ChangeSceneToFile("res://src/ui/menus/HostSetupMenu.tscn");
        }).CallDeferred();
    }

    private async void OnPromote(int slotIndex)
    {
        var ctx = SessionContext.Instance;
        if (slotIndex < 0 || slotIndex >= ctx.PlayerIds.Count)
            return;
        var target = ctx.PlayerIds[slotIndex];
        if (target == ctx.HostPlayerId)
            return;

        ShowStatus($"Promoting {ctx.DisplayNameFor(target)}…");
        var result = await ctx.Api.TransferHostAsync(
            ctx.SessionCode, ctx.PlayerId, ctx.SecretToken, target);
        if (!result.Ok)
            Callable.From(() => ShowStatus($"Promote failed: {result.ErrorCode}")).CallDeferred();
        // Success → server broadcasts host_changed; OnHostChanged updates all clients.
    }

    // ---- rendering ----

    private void Render()
    {
        var ctx = SessionContext.Instance;

        GetNode<Label>("%SessionCodeValue").Text =
            string.IsNullOrEmpty(ctx.SessionCode) ? "------" : ctx.SessionCode;
        GetNode<Label>("%ModeValue").Text =
            string.IsNullOrEmpty(ctx.GameMode) ? "—" : ctx.GameMode;
        GetNode<Label>("%MaxPlayersValue").Text = ctx.MaxPlayers.ToString();

        for (int i = 0; i < _slots.Length; i++)
        {
            var slot = _slots[i];
            bool slotUsed = i < ctx.PlayerIds.Count;
            string pid = slotUsed ? ctx.PlayerIds[i] : "";
            bool slotIsHost = slotUsed && pid == ctx.HostPlayerId;

            slot.GetNode<Label>("Name").Text = slotUsed ? ctx.DisplayNameFor(pid) : "Empty Slot";
            slot.GetNode<Label>("IsHost").Visible = slotIsHost;
            slot.GetNode<Button>("Promote").Visible = slotUsed && ctx.LocalPlayerIsHost && !slotIsHost;
        }

        GetNode<Button>("%StartSessionButton").Visible = ctx.LocalPlayerIsHost;
        GetNode<Button>("%ReturnToSetupButton").Visible = ctx.LocalPlayerIsHost;
        GetNode<Button>("%CancelSessionButton").Text =
            ctx.LocalPlayerIsHost ? "Cancel Session" : "Leave Session";
    }

    private void ShowStatus(string message)
    {
        _status.Text = message;
        _status.Visible = !string.IsNullOrEmpty(message);
    }

    private void LeaveToMenu(string message)
    {
        if (_leaving)
            return;
        _leaving = true;
        if (!string.IsNullOrEmpty(message))
            GD.Print($"[lobby] {message}");
        _signaling.CloseConnection();
        SessionContext.Instance.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }
}
