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
    // Moderator chat lines are tinted so staff never read as another player.
    // The panel's own MOD badge uses the same blue (#58a6ff).
    private static readonly Color ModeratorChatColor = new("58a6ff");

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

        // Lobby chat: Enter in the input sends (LineEdit.TextSubmitted). The log
        // starts empty (drop the scene's sample line) and fills only from
        // server-broadcast chat_message frames so every client shows the same
        // transcript.
        GetNode<LineEdit>("%ChatInput").TextSubmitted += OnChatSubmitted;
        GetNode<RichTextLabel>("%ChatLog").Clear();

        _status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
        GetNode("LeftPanel/LeftMargins/LeftContent").AddChild(_status);

        // Hand the lifecycle to the orchestrator: it opens the signaling socket,
        // owns the roster mutations, and will swap scenes on start_signaling.
        var flow = MatchFlow.Instance;
        flow.EnterLobby();
        flow.RosterChanged += Render;

        // Pure-UI signaling events stay view-subscribed (chat lines, transient
        // reconnect status). Lifecycle events are MatchFlow's alone.
        _signaling = flow.Signaling;
        if (_signaling != null)
        {
            _signaling.ChatMessage += OnChatMessage;
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
            _signaling.ChatMessage -= OnChatMessage;
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

    // Enter pressed in the chat input. Trim, send through the orchestrator (the
    // server echoes it back to everyone, including us, so we render on receipt
    // rather than locally — keeping all clients' transcripts identical), then
    // clear the field. Empty/whitespace input is dropped without a round-trip.
    private void OnChatSubmitted(string text)
    {
        var input = GetNode<LineEdit>("%ChatInput");
        var trimmed = text.Trim();
        if (trimmed.Length > 0)
            MatchFlow.Instance.SendChat(trimmed);
        input.Clear();
    }

    // A chat line broadcast by the server (ours or a peer's). Keep the roster's
    // name map in step with what chat reports, then label via the same
    // DisplayNameFor fallback (Player <id>) the rest of the lobby uses.
    //
    // A moderator line is the exception: it carries no sender id, so it must not
    // go through the roster at all — feeding it an empty id would register a
    // phantom roster entry and then label the line from it.
    private void OnChatMessage(Net.ChatLine line)
    {
        if (line.IsModerator)
        {
            AppendModeratorLine(line.Username, line.Text);
            return;
        }

        var ctx = SessionContext.Instance;
        if (!string.IsNullOrEmpty(line.Username))
            ctx.SetUsername(line.From, line.Username);
        AppendChatLine(ctx.DisplayNameFor(line.From), line.Text);
    }

    // Append one "<name>: <text>" line. Uses AddText (not AppendText) for the
    // server- and user-supplied strings so they are never parsed as BBCode —
    // only the bold tag around the name is pushed by us, so no tag injection.
    private void AppendChatLine(string name, string text)
    {
        var log = GetNode<RichTextLabel>("%ChatLog");
        log.PushBold();
        log.AddText(name);
        log.Pop();
        log.AddText($": {text}\n");
    }

    // A moderator speaking into the session: "[MOD] <name>: <text>", tinted so it
    // reads as staff rather than another player. Same rule as AppendChatLine —
    // the colour and bold are pushed by us via the API, never interpolated as
    // BBCode into a string, so the moderator's name and text still cannot inject
    // tags. The name here is whatever the moderator chose to appear as; the
    // client is deliberately not told who is behind an anonymous "Mod".
    private void AppendModeratorLine(string name, string text)
    {
        var log = GetNode<RichTextLabel>("%ChatLog");
        log.PushColor(ModeratorChatColor);
        log.PushBold();
        log.AddText($"[MOD] {name}");
        log.Pop();
        log.AddText($": {text}\n");
        log.Pop();
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
