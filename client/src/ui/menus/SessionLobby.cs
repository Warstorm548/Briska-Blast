using System.Collections.Generic;
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

    // A warning aimed at this player. Amber rather than the moderator blue: it is
    // not someone talking, it is an action taken against them.
    private static readonly Color WarningChatColor = new("d29922");

    /// <summary>
    /// One line this client received, kept so the log can be rebuilt.
    ///
    /// The chat log is a single <see cref="RichTextLabel"/> buffer with no
    /// per-line nodes, so there is nothing in the scene tree to address when a
    /// moderator deletes a message. This list is that address book.
    /// </summary>
    private sealed class LogEntry
    {
        public string BodyId = "";
        public string Name = "";
        public string Text = "";
        public bool IsModerator;
        public bool IsWarning;
        /// <summary>Deleted by a moderator: the text is wiped and rendering skips
        /// it, but the entry stays in place. Holding the position means a future
        /// restore could refill it where it belongs without the server having to
        /// describe where that is.</summary>
        public bool Deleted;
    }

    // Ordered oldest-first, mirroring what the log displays. Lives only as long
    // as this scene — leaving the lobby discards it, which is also the window in
    // which any restore would have to happen.
    private readonly List<LogEntry> _log = new();

    private HBoxContainer[] _slots = null!;
    private Label _status = null!;

    // Built on first use — a player who is never warned never gets one.
    private Control? _warnBanner;
    private Label? _warnText;

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
            _signaling.ChatWarning += OnChatWarning;
            _signaling.ChatBodyDeleted += OnChatBodyDeleted;
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
            _signaling.ChatWarning -= OnChatWarning;
            _signaling.ChatBodyDeleted -= OnChatBodyDeleted;
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
            Append(new LogEntry
            {
                BodyId = line.BodyId,
                Name = line.Username,
                Text = line.Text,
                IsModerator = true,
            });
            return;
        }

        var ctx = SessionContext.Instance;
        if (!string.IsNullOrEmpty(line.Username))
            ctx.SetUsername(line.From, line.Username);
        Append(new LogEntry
        {
            BodyId = line.BodyId,
            Name = ctx.DisplayNameFor(line.From),
            Text = line.Text,
        });
    }

    // A moderator warned this player. Only this client receives it, and it is
    // never queued — so it is shown twice over: in the log where the conversation
    // is, and on a banner that cannot scroll away.
    private void OnChatWarning(string reason)
    {
        Append(new LogEntry { Text = reason, IsWarning = true });
        ShowWarningBanner(reason);
    }

    // A moderator withdrew a message from everyone in the session.
    //
    // The entry is emptied, not removed: rendering skips it so the visible log
    // closes up with no gap, while the body id keeps its place in the order. A
    // later restore could then refill it exactly where it was without the server
    // needing to say where that is.
    private void OnChatBodyDeleted(string bodyId)
    {
        if (string.IsNullOrEmpty(bodyId))
            return;
        foreach (var entry in _log)
        {
            // An unrecorded line carries no id, so an empty id must never match.
            if (entry.BodyId != bodyId)
                continue;
            entry.Text = "";
            entry.Name = "";
            entry.Deleted = true;
            RedrawLog();
            return;
        }
        // Not found: this client joined after the message, or never saw it.
    }

    // Record a line and draw it. Appending rather than redrawing keeps the common
    // case cheap and leaves RichTextLabel's scroll_following to do its job.
    private void Append(LogEntry entry)
    {
        _log.Add(entry);
        DrawEntry(GetNode<RichTextLabel>("%ChatLog"), entry);
    }

    // Rebuild from the kept list. Only a deletion needs this — every other change
    // is an append.
    private void RedrawLog()
    {
        var log = GetNode<RichTextLabel>("%ChatLog");
        log.Clear();
        foreach (var entry in _log)
            DrawEntry(log, entry);
    }

    // Draw one entry. Every server- and user-supplied string goes through AddText
    // (never AppendText), and colour/bold come from the Push*/Pop API rather than
    // interpolated BBCode — the label has bbcode_enabled, so building tags into a
    // string would be a tag-injection hole.
    private static void DrawEntry(RichTextLabel log, LogEntry entry)
    {
        // A deleted line leaves no trace in the log: the placeholder exists to
        // hold its position in the list, not to show a gap on screen.
        if (entry.Deleted)
            return;

        if (entry.IsWarning)
        {
            log.PushColor(WarningChatColor);
            log.PushBold();
            log.AddText("[WARNING]");
            log.Pop();
            log.AddText($" {entry.Text}\n");
            log.Pop();
            return;
        }

        if (entry.IsModerator)
        {
            // The name is whatever the moderator chose to appear as; the client
            // is deliberately not told who is behind an anonymous "Mod".
            log.PushColor(ModeratorChatColor);
            log.PushBold();
            log.AddText($"[MOD] {entry.Name}");
            log.Pop();
            log.AddText($": {entry.Text}\n");
            log.Pop();
            return;
        }

        log.PushBold();
        log.AddText(entry.Name);
        log.Pop();
        log.AddText($": {entry.Text}\n");
    }

    // A dismissible banner pinned at the top of the chat panel. A warning that
    // only appeared in the log would scroll away behind the next few messages,
    // and there is no second copy — the server does not resend it.
    private void ShowWarningBanner(string reason)
    {
        if (_warnBanner == null)
        {
            var chat = GetNode<VBoxContainer>(
                "RightPanel/RightMargins/RightContent/ChatBox/ChatMargins/ChatVBox");
            var row = new HBoxContainer();
            _warnText = new Label
            {
                AutowrapMode = TextServer.AutowrapMode.WordSmart,
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
            };
            _warnText.AddThemeColorOverride("font_color", WarningChatColor);
            var dismiss = new Button { Text = "✕" };
            dismiss.Pressed += () => row.Visible = false;
            row.AddChild(_warnText);
            row.AddChild(dismiss);
            chat.AddChild(row);
            chat.MoveChild(row, 0);
            _warnBanner = row;
        }

        // Replace rather than stack: a second warning is the current one, and a
        // growing pile of banners would push the chat itself off screen.
        _warnText!.Text = $"⚠ Warning from a moderator: {reason}";
        _warnBanner.Visible = true;
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
