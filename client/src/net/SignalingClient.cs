using System.Text;
using System.Text.Json;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>
/// One player's signaling connection for one session. A <see cref="Node"/>
/// so it can poll the <see cref="WebSocketPeer"/> every frame in
/// <see cref="_Process"/> — which means all events below fire on Godot's
/// main thread and handlers may touch the scene tree directly.
///
/// Sends <c>identify</c> on open and <c>leave</c> on request, and surfaces the
/// lifecycle frames: Identified / PeerJoined / PeerLeft / HostChanged /
/// HostReconnecting / HostReconnected / PeerReconnecting / StartSignaling /
/// SessionEnded / Kicked / Closed, plus ScoreUpdate and the WebRTC relays
/// (offer/answer/ice_candidate) the transport consumes.
///
/// On an <em>unexpected</em> drop it does not give up: it re-dials the same
/// session WS (re-sending <c>identify</c>) for a short window — surfacing
/// Reconnecting / Reconnected — so a transient blip doesn't end the match and
/// the server's host-reconnect grace is actually reachable. Only a deliberate
/// close or an auth-level rejection is terminal (Closed).
/// </summary>
public partial class SignalingClient : Node
{
    private WebSocketPeer _ws = new();
    private string _code = "";
    private string _playerId = "";
    private string _secretToken = "";
    private bool _active;
    private bool _identifySent;
    private bool _closedEmitted;

    /// <summary>Declare this connection's identifies as a process-death rejoin
    /// into a live match — the server then pauses the match while this client
    /// re-meshes. Set by <see cref="Core.MatchFlow"/> only on its rejoin paths
    /// (never for a lobby connect) and cleared once the client is in-match, so
    /// a later transient WS auto-reconnect re-identifies as a normal member and
    /// can't wrongly pause everyone.</summary>
    public bool IdentifyAsRejoin { get; set; }

    /// <summary>Open the WS for <paramref name="code"/> and begin identifying.</summary>
    public void Connect(string code, string playerId, string secretToken)
    {
        _code = code;
        _playerId = playerId;
        _secretToken = secretToken;

        // Reset lifecycle flags so a reused instance re-sends identify and can
        // emit Closed again, rather than carrying stale state from a prior run.
        _identifySent = false;
        _closedEmitted = false;
        _active = false;
        _closing = false;
        _reconnecting = false;

        var url = $"{ServerEndpoint.WsBase}/ws/session/{code}";
        var err = _ws.ConnectToUrl(url);
        if (err != Error.Ok)
        {
            Log.Warn("net.signaling", $"ConnectToUrl({url}) failed: {err}");
            EmitClosedOnce(0, $"connect_failed:{err}");
            return;
        }
        _active = true;
    }

    /// <summary>Send a voluntary <c>leave</c> frame (host hands off / joiner
    /// leaves the lobby). The server frees the slot when in Waiting.</summary>
    public void SendLeave()
    {
        // Deliberate departure: never auto-reconnect after this.
        _closing = true;
        if (_ws.GetReadyState() != WebSocketPeer.State.Open)
            return;
        _ws.SendText(JsonSerializer.Serialize(new LeaveFrame("leave"), Json.Options));
        // Flush now: the lobby tears the socket down immediately after this, so
        // without an explicit Poll the queued frame can be dropped before it
        // reaches the wire — and the server would then see a plain disconnect
        // (slot kept for reconnect) instead of an explicit leave (slot freed).
        _ws.Poll();
    }

    /// <summary>Initiate a clean close. Safe to call repeatedly.</summary>
    public void CloseConnection()
    {
        // Deliberate close: suppress the reconnect loop.
        _closing = true;
        if (_active)
            _ws.Close();
    }

    private void SendFrame<T>(T frame)
    {
        if (_ws.GetReadyState() == WebSocketPeer.State.Open)
            _ws.SendText(JsonSerializer.Serialize(frame, Json.Options));
    }

    public override void _Process(double delta)
    {
        if (!_active)
            return;

        _ws.Poll();
        var state = _ws.GetReadyState();

        // Deliberate close in progress: don't reconnect — just surface the
        // terminal close once the socket finishes closing.
        if (_closing)
        {
            if (state == WebSocketPeer.State.Closed)
                EmitClosedOnce(_ws.GetCloseCode(), _ws.GetCloseReason());
            return;
        }

        if (_reconnecting)
        {
            ProcessReconnect(state);
            return;
        }

        switch (state)
        {
            case WebSocketPeer.State.Open:
                SendIdentifyOnce();
                MaybeSendTimeSync();
                DrainPackets();
                break;

            case WebSocketPeer.State.Closed:
                OnSocketClosed();
                break;
        }
    }

    private void SendIdentifyOnce()
    {
        if (_identifySent)
            return;
        _ws.SendText(JsonSerializer.Serialize(
            new IdentifyFrame("identify", _playerId, _secretToken, IdentifyAsRejoin), Json.Options));
        _identifySent = true;
    }

    private void DrainPackets()
    {
        while (_ws.GetAvailablePacketCount() > 0)
            HandleFrame(Encoding.UTF8.GetString(_ws.GetPacket()));
    }
}
