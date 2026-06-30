using Godot;
using BriskaBlast.Core;
using BriskaBlast.Net;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// Live lobby. Opens a signaling WebSocket on entry, drives the roster from
/// server events (Identified / PeerJoined / PeerLeft / HostChanged), and
/// backs the buttons with the REST endpoints. On <c>start_signaling</c> it
/// builds the WebRTC mesh, hands the live signaling + transport to
/// <see cref="SessionContext"/> (so they survive the scene change), and enters
/// <c>GameScene</c>.
/// </summary>
public partial class SessionLobby : Control
{
    private HBoxContainer[] _slots = null!;
    private SignalingClient _signaling = null!;
    private Label _status = null!;
    private bool _leaving;

    // Built on start_signaling, then handed to SessionContext (AdoptNet) so it
    // outlives this scene. Null until then.
    private WebRtcMeshTransport? _transport;

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

        // Lobby chat: Enter in the input sends (LineEdit.TextSubmitted). The log
        // starts empty (drop the scene's sample line) and fills only from
        // server-broadcast chat_message frames so every client shows the same
        // transcript.
        GetNode<LineEdit>("%ChatInput").TextSubmitted += OnChatSubmitted;
        GetNode<RichTextLabel>("%ChatLog").Clear();

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
        _signaling.SessionEnded += OnSessionEnded;
        _signaling.Kicked += OnKicked;
        _signaling.Closed += OnClosed;
        _signaling.Reconnecting += OnReconnecting;
        _signaling.Reconnected += OnReconnected;
        _signaling.ChatMessage += OnChatMessage;

        var ctx = SessionContext.Instance;
        _signaling.Connect(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);

        Render();
    }

    // ---- signaling callbacks (main thread) ----

    /// <summary>Handle the lobby's <c>Identified</c> frame: refresh usernames, the
    /// host, and the roster from the server's snapshot and re-render. (seatOrder is
    /// empty before Start, so it's captured later in <see cref="OnStartSignaling"/>.)
    /// </summary>
    private void OnIdentified(string hostId, string[] peers, string[] seatOrder,
        bool isHost, System.Collections.Generic.Dictionary<string, string> usernames)
    {
        // seatOrder is empty in the lobby (it's frozen at Start); the seating
        // roster is captured in OnStartSignaling. Nothing to do with it here.
        var ctx = SessionContext.Instance;
        ctx.MergeUsernames(usernames);
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

    private void OnPeerJoined(string playerId, string username)
    {
        var ctx = SessionContext.Instance;
        ctx.SetUsername(playerId, username);
        if (!ctx.PlayerIds.Contains(playerId))
            ctx.PlayerIds.Add(playerId);
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

    // The WS dropped but the client is re-dialing (transient blip) rather than
    // leaving — surface it on the status line instead of bouncing to the menu.
    private void OnReconnecting() => ShowStatus("Reconnecting…");

    private void OnReconnected() => ShowStatus("");

    // Enter pressed in the chat input. Trim, send through signaling (the server
    // echoes it back to everyone, including us, so we render on receipt rather
    // than locally — keeping all clients' transcripts identical), then clear the
    // field. Empty/whitespace input is dropped without a round-trip.
    private void OnChatSubmitted(string text)
    {
        var input = GetNode<LineEdit>("%ChatInput");
        var trimmed = text.Trim();
        if (trimmed.Length > 0 && _signaling != null)
            _signaling.SendChatMessage(trimmed);
        input.Clear();
    }

    // A chat line broadcast by the server (ours or a peer's). Keep the roster's
    // name map in step with what chat reports, then label via the same
    // DisplayNameFor fallback (Player <id>) the rest of the lobby uses.
    private void OnChatMessage(string from, string username, string text)
    {
        var ctx = SessionContext.Instance;
        if (!string.IsNullOrEmpty(username))
            ctx.SetUsername(from, username);
        AppendChatLine(ctx.DisplayNameFor(from), text);
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

    /// <summary>Handle <c>start_signaling</c>: freeze the seating roster, bring up
    /// the WebRTC mesh, hand the live net to <see cref="SessionContext"/>, and enter
    /// the game scene. One-shot — guards against a duplicate frame.</summary>
    private void OnStartSignaling(string gamemode, WinConditionDto winCondition,
        SpawnSettingsDto spawnSettings, int playerCount, string[] peers)
    {
        // One-shot transition: guard against a duplicate start_signaling and
        // against firing while we're already leaving.
        if (_transport != null || _leaving)
            return;

        GetNode<Button>("%StartSessionButton").Disabled = true;
        ShowStatus("Starting…");

        var ctx = SessionContext.Instance;

        // Adopt the authoritative win condition + random-spawn rules from the start
        // frame (the host's own already match; a joiner learns them here) so the game
        // scene applies the same rules the server enforces / broadcasts.
        ctx.ApplyWinCondition(winCondition);
        ctx.ApplySpawnSettings(spawnSettings);

        // Freeze the seating roster for Extended-mode portal layout. `peers` here
        // is the server's authoritative, self-inclusive start-time roster
        // ([host, …joiners] in join order) — identical on every client — so each
        // screen lays out portals consistently. See GameScene.BuildEdges.
        ctx.SetSeatOrder(peers);

        // Establish the WebRTC mesh to every peer. Negotiation rides the
        // signaling socket and continues in the background after the transition
        // (both nodes keep polling under the SessionContext autoload).
        if (peers.Length > 0)
        {
            _transport = new WebRtcMeshTransport();
            _transport.Init(_signaling);
            AddChild(_transport);
            _transport.Connect(ctx.PlayerId, peers);
        }

        // Hand the live signaling + transport to SessionContext so they survive
        // ChangeSceneToFile, then enter the game. Unsubscribe our own handlers
        // first: the socket lives on, but this lobby is about to be freed.
        UnsubscribeSignaling();
        ctx.AdoptNet(_signaling, _transport);
        _signaling = null!;
        _transport = null;
        _leaving = true; // handing off, not tearing down

        // If the scene change fails we've already handed off our signaling, so
        // this lobby can't continue safely — tear the net down and fall back to
        // the main menu rather than linger with a null socket.
        if (GetTree().ChangeSceneToFile("res://src/game/GameScene.tscn") != Error.Ok)
        {
            GD.PushError("[lobby] failed to enter GameScene — returning to menu.");
            ctx.TeardownNet();
            ctx.ClearSession();
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
        }
    }

    /// <summary>Detach every signaling handler this lobby installed. The socket
    /// is being handed to SessionContext and keeps emitting; this scene must not
    /// be called after it is freed. (The transport's own offer/answer/ice
    /// subscriptions live on the transport node and travel with it.)</summary>
    private void UnsubscribeSignaling()
    {
        _signaling.Identified -= OnIdentified;
        _signaling.PeerJoined -= OnPeerJoined;
        _signaling.PeerLeft -= OnPeerLeft;
        _signaling.HostChanged -= OnHostChanged;
        _signaling.StartSignaling -= OnStartSignaling;
        _signaling.SessionEnded -= OnSessionEnded;
        _signaling.Kicked -= OnKicked;
        _signaling.Closed -= OnClosed;
        _signaling.Reconnecting -= OnReconnecting;
        _signaling.Reconnected -= OnReconnected;
        _signaling.ChatMessage -= OnChatMessage;
    }

    private void OnSessionEnded(string reason) => LeaveToMenu($"Session ended ({reason}).");

    private void OnKicked(string reason) => LeaveToMenu($"Removed from session ({reason}).");

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
        _transport?.Close();
        _signaling.CloseConnection();
        SessionContext.Instance.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }
}
