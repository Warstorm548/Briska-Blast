using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// Live lobby — a thin view over <see cref="MatchFlow"/>. On entry it hands the
/// lifecycle to the orchestrator (<see cref="MatchFlow.EnterLobby"/>), renders
/// the roster whenever MatchFlow reports it changed, and backs the buttons with
/// the REST endpoints. Everything stateful — the signaling socket, the start
/// choreography, teardown, scene transitions — lives in MatchFlow; this scene
/// only subscribes to the pure-UI signaling events (chat, reconnect status).
/// </summary>
public partial class SessionLobby : Control
{
    private HBoxContainer[] _slots = null!;
    private Label _status = null!;

    // The signaling instance this scene subscribed to for pure-UI events.
    // Captured (not re-read from MatchFlow) so _ExitTree unsubscribes from the
    // same object even after a teardown already nulled MatchFlow.Signaling.
    private Net.SignalingClient? _signaling;

    public override void _Ready()
    {
        _slots = new[]
        {
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/RosterBox/RosterMargins/PlayerSlots/Slot1"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/RosterBox/RosterMargins/PlayerSlots/Slot2"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/RosterBox/RosterMargins/PlayerSlots/Slot3"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/RosterBox/RosterMargins/PlayerSlots/Slot4"),
        };

        for (int i = 0; i < _slots.Length; i++)
        {
            var idx = i;
            _slots[i].GetNode<Button>("Promote").Pressed += () => OnPromote(idx);
        }

        GetNode<Button>("%StartSessionButton").Pressed += OnStartPressed;
        GetNode<Button>("%CancelSessionButton").Pressed += OnCancelOrLeavePressed;
        GetNode<Button>("%ReturnToSetupButton").Pressed += OnReturnToSetupPressed;
        GetNode<TextureButton>("%CopyCodeButton").Pressed += OnCopyCode;

        _status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
        GetNode("LeftPanel/LeftMargins/LeftContent").AddChild(_status);

        // Hand the lifecycle to the orchestrator: it opens the signaling socket,
        // owns the roster mutations, and will swap scenes on start_signaling.
        var flow = MatchFlow.Instance;
        flow.EnterLobby();
        flow.RosterChanged += Render;

        // Chat is recorded by MatchFlow (it has to outlive this scene), so the
        // panel only binds the shared transcript — drawing what is already there
        // and following it from here on.
        GetNode<Chat.ChatPanel>("RightPanel/RightMargins/RightContent/ChatBox")
            .Bind(flow.Chat);

        // Pure-UI signaling events stay view-subscribed (transient reconnect
        // status). Lifecycle events are MatchFlow's alone.
        _signaling = flow.Signaling;
        if (_signaling != null)
        {
            _signaling.Reconnecting += OnReconnecting;
            _signaling.Reconnected += OnReconnected;
        }

        Render();
    }

    public override void _ExitTree()
    {
        // Detach from the orchestrator and the surviving socket so neither
        // calls into a freed scene (the socket lives on across the change to
        // the Preparing/game scenes).
        MatchFlow.Instance.RosterChanged -= Render;
        if (_signaling != null)
        {
            _signaling.Reconnecting -= OnReconnecting;
            _signaling.Reconnected -= OnReconnected;
            _signaling = null;
        }
    }

    /// <summary>Copy the session code to the OS clipboard and flash a brief confirmation.</summary>
    private void OnCopyCode() =>
        ClipboardCopy.CopyWithFlash(GetTree(), SessionContext.Instance?.SessionCode,
            GetNode<Label>("%CopiedFlash"));

    // ---- pure-UI signaling callbacks (main thread) ----

    // The WS dropped but the client is re-dialing (transient blip) rather than
    // leaving — surface it on the status line instead of bouncing to the menu.
    private void OnReconnecting() => ShowStatus("Reconnecting…");

    private void OnReconnected() => ShowStatus("");

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
        // Success → the server broadcasts start_signaling; MatchFlow handles it
        // (rule adoption, seat freeze, mesh, and the scene change).
    }

    private async void OnCancelOrLeavePressed()
    {
        var ctx = SessionContext.Instance;
        if (ctx.LocalPlayerIsHost)
        {
            // Host cancels → tear the session down server-side, then leave
            // locally. No leave frame — the session is being deleted anyway.
            // Only leave once the server actually closed it; otherwise stay so
            // the lobby doesn't diverge from a still-live session.
            var result = await ctx.Api.CloseSessionAsync(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
            Callable.From(() =>
            {
                if (result.Ok)
                    MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);
                else
                    ShowStatus($"Could not close the session: {result.ErrorCode}");
            }).CallDeferred();
        }
        else
        {
            // Joiner leaves → explicit Leave frees the slot in the lobby.
            MatchFlow.Instance.LeaveSession(sendLeaveFrame: true);
        }
    }

    private async void OnReturnToSetupPressed()
    {
        var ctx = SessionContext.Instance;
        var result = await ctx.Api.CloseSessionAsync(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
        Callable.From(() =>
        {
            if (result.Ok)
                MatchFlow.Instance.EndMatchTo("res://src/ui/menus/HostSetupMenu.tscn");
            else
                ShowStatus($"Could not close the session: {result.ErrorCode}");
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
        // Success → server broadcasts host_changed; MatchFlow updates the
        // roster and fires RosterChanged on every client.
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
}
