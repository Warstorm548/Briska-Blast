using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using Godot;

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
    /// <summary>Identify accepted. Carries (hostPlayerId, peers, selfIsHost).</summary>
    public event Action<string, string[], bool>? Identified;
    public event Action<string>? PeerJoined;
    public event Action<string, string>? PeerLeft;
    public event Action<string>? HostChanged;
    public event Action<string, int, string[]>? StartSignaling;
    public event Action<string>? SessionEnded;
    public event Action<string>? Kicked;
    /// <summary>Authoritative per-session score tally (player_id → points)
    /// broadcast by the server after a score report. Overwrite, don't add.</summary>
    public event Action<Dictionary<string, int>>? ScoreUpdate;
    /// <summary>Socket closed for good (deliberate close, an auth-level
    /// rejection, or the reconnect window expired). Carries the close code
    /// (4xxx app codes from the server, or transport codes like 1006) and the
    /// reason string.</summary>
    public event Action<int, string>? Closed;
    /// <summary>The WS dropped unexpectedly and an automatic reconnect loop has
    /// begun — the session is not abandoned yet, so UI may show a
    /// "reconnecting…" state instead of leaving.</summary>
    public event Action? Reconnecting;
    /// <summary>The automatic reconnect succeeded and <c>identify</c> was
    /// re-sent; the session resumes on the same connection.</summary>
    public event Action? Reconnected;
    /// <summary>The host dropped mid-game and the server armed a reconnect grace
    /// window. Carries (hostPlayerId, graceSecs).</summary>
    public event Action<string, int>? HostReconnecting;
    /// <summary>A dropped host returned within the grace window; the host role
    /// is unchanged.</summary>
    public event Action<string>? HostReconnected;
    /// <summary>A non-host peer dropped mid-game and the server armed a reconnect
    /// window. Carries (playerId, graceSecs) so peers can show a
    /// "reconnecting…" overlay. Resolved by <see cref="PeerJoined"/> (they
    /// rejoined) or <see cref="PeerLeft"/> (the window elapsed).</summary>
    public event Action<string, int>? PeerReconnecting;

    // WebRTC negotiation relays (server attests `from`). The transport layer
    // subscribes to these and feeds them into the matching peer connection.
    public event Action<string, string>? OfferReceived;          // (from, sdp)
    public event Action<string, string>? AnswerReceived;         // (from, sdp)
    public event Action<string, string, string, int>? IceCandidateReceived; // (from, candidate, sdpMid, sdpMLineIndex)

    private WebSocketPeer _ws = new();
    private string _code = "";
    private string _playerId = "";
    private string _secretToken = "";
    private bool _active;
    private bool _identifySent;
    private bool _closedEmitted;

    // Reconnect state. On an unexpected drop the client re-dials the same
    // session WS (re-sending identify) for up to ReconnectWindowMsec before
    // giving up and surfacing Closed. A deliberate close (_closing) never
    // reconnects. This window is what makes the server's host grace reachable.
    private bool _closing;
    private bool _reconnecting;
    private ulong _reconnectStartMsec;
    private ulong _nextAttemptMsec;
    private const ulong ReconnectWindowMsec = 30_000; // ~ the server's host grace
    private const ulong RetryIntervalMsec = 2_000;

    private sealed record IdentifyFrame(string Type, string PlayerId, string SecretToken);
    private sealed record LeaveFrame(string Type);
    private sealed record OfferFrame(string Type, string To, string Sdp);
    private sealed record AnswerFrame(string Type, string To, string Sdp);
    private sealed record IceFrame(string Type, string To, string Candidate, string SdpMid, int SdpMLineIndex);
    private sealed record PeerConnectionFailedFrame(string Type, string Peer, string Reason);
    private sealed record ReportScoreFrame(string Type, string ScoringPlayerId);

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
            GD.PushWarning($"[signaling] ConnectToUrl({url}) failed: {err}");
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

    // WebRTC negotiation senders. `to` is the target peer's player_id; the
    // server attests `from` from this authenticated connection.
    public void SendOffer(string to, string sdp) => SendFrame(new OfferFrame("offer", to, sdp));

    public void SendAnswer(string to, string sdp) => SendFrame(new AnswerFrame("answer", to, sdp));

    public void SendIceCandidate(string to, string candidate, string sdpMid, int sdpMLineIndex) =>
        SendFrame(new IceFrame("ice_candidate", to, candidate, sdpMid, sdpMLineIndex));

    /// <summary>Report that a direct connection to <paramref name="peer"/>
    /// could not be established (e.g. ICE exhausted). The server logs it.</summary>
    public void SendPeerConnectionFailed(string peer, string reason) =>
        SendFrame(new PeerConnectionFailedFrame("peer_connection_failed", peer, reason));

    /// <summary>Report that <paramref name="scoringPlayerId"/> (the last player
    /// to hit the ball) scored. The server tallies and broadcasts ScoreUpdate.</summary>
    public void SendReportScore(string scoringPlayerId) =>
        SendFrame(new ReportScoreFrame("report_score", scoringPlayerId));

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
            new IdentifyFrame("identify", _playerId, _secretToken), Json.Options));
        _identifySent = true;
    }

    private void DrainPackets()
    {
        while (_ws.GetAvailablePacketCount() > 0)
            HandleFrame(Encoding.UTF8.GetString(_ws.GetPacket()));
    }

    /// <summary>Unexpected close while in a session. App-level rejections
    /// (auth / not in session) are terminal — retrying can't help; anything
    /// else (transport blip, server restart) starts the reconnect loop.</summary>
    private void OnSocketClosed()
    {
        int code = _ws.GetCloseCode();
        if (IsTerminalClose(code))
        {
            EmitClosedOnce(code, _ws.GetCloseReason());
            return;
        }
        _reconnecting = true;
        _reconnectStartMsec = Time.GetTicksMsec();
        _nextAttemptMsec = 0; // first retry on the next tick
        GD.Print($"[signaling] connection lost (code {code}) — reconnecting…");
        Reconnecting?.Invoke();
    }

    private void ProcessReconnect(WebSocketPeer.State state)
    {
        switch (state)
        {
            case WebSocketPeer.State.Open:
                // Back up: re-identify on the fresh socket and resume.
                SendIdentifyOnce();
                _reconnecting = false;
                GD.Print("[signaling] reconnected.");
                Reconnected?.Invoke();
                DrainPackets();
                break;

            case WebSocketPeer.State.Connecting:
                break; // still dialing

            case WebSocketPeer.State.Closed:
                // A reconnect attempt rejected at the app level (auth / not in
                // this session — e.g. an ex-host promoted away) can't succeed by
                // retrying, so bail immediately rather than spin out the window.
                int code = _ws.GetCloseCode();
                if (IsTerminalClose(code))
                {
                    _reconnecting = false;
                    EmitClosedOnce(code, _ws.GetCloseReason());
                    return;
                }
                ulong now = Time.GetTicksMsec();
                if (now - _reconnectStartMsec >= ReconnectWindowMsec)
                {
                    _reconnecting = false;
                    EmitClosedOnce(_ws.GetCloseCode(), "reconnect_failed");
                    return;
                }
                if (now >= _nextAttemptMsec)
                {
                    AttemptReconnect();
                    _nextAttemptMsec = now + RetryIntervalMsec;
                }
                break;
        }
    }

    /// <summary>Dial a fresh peer (no stale close-state lingering); identify
    /// re-sends once it opens. A failed dial just waits for the next interval.</summary>
    private void AttemptReconnect()
    {
        _ws = new WebSocketPeer();
        _identifySent = false;
        var url = $"{ServerEndpoint.WsBase}/ws/session/{_code}";
        var err = _ws.ConnectToUrl(url);
        if (err != Error.Ok)
            GD.PushWarning($"[signaling] reconnect dial failed: {err}");
    }

    // App close codes that mean "you can't be in this session" — reconnecting
    // is futile. Transport codes (e.g. 1006) and -1 (abnormal) are transient.
    private static bool IsTerminalClose(int code) =>
        code == 4401 || code == 4403 || code == 4404;

    private void EmitClosedOnce(int code, string reason)
    {
        if (_closedEmitted)
            return;
        _closedEmitted = true;
        _active = false;
        Closed?.Invoke(code, reason);
    }

    private void HandleFrame(string text)
    {
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (!root.TryGetProperty("type", out var typeEl))
                return;

            switch (typeEl.GetString())
            {
                case "identified":
                    Identified?.Invoke(
                        Str(root, "host_player_id"),
                        ReadStrings(root, "peers"),
                        root.GetProperty("is_host").GetBoolean());
                    break;
                case "peer_joined":
                    PeerJoined?.Invoke(Str(root, "player_id"));
                    break;
                case "peer_left":
                    PeerLeft?.Invoke(Str(root, "player_id"), Str(root, "reason"));
                    break;
                case "host_changed":
                    HostChanged?.Invoke(Str(root, "player_id"));
                    break;
                case "host_reconnecting":
                    HostReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "host_reconnected":
                    HostReconnected?.Invoke(Str(root, "player_id"));
                    break;
                case "peer_reconnecting":
                    PeerReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "start_signaling":
                    StartSignaling?.Invoke(
                        Str(root, "gamemode"),
                        root.GetProperty("player_count").GetInt32(),
                        ReadStrings(root, "peers"));
                    break;
                case "session_ended":
                    SessionEnded?.Invoke(Str(root, "reason"));
                    break;
                case "kicked":
                    Kicked?.Invoke(Str(root, "reason"));
                    break;
                case "score_update":
                    ScoreUpdate?.Invoke(ReadIntMap(root, "scores"));
                    break;
                case "offer":
                    OfferReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "answer":
                    AnswerReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "ice_candidate":
                    IceCandidateReceived?.Invoke(
                        Str(root, "from"),
                        Str(root, "candidate"),
                        Str(root, "sdp_mid"),
                        IntProp(root, "sdp_m_line_index"));
                    break;
                default:
                    GD.Print($"[signaling] ignoring unknown frame type '{typeEl.GetString()}'");
                    break;
            }
        }
        catch (Exception e)
        {
            GD.PushWarning($"[signaling] failed to parse frame: {e.Message}");
        }
    }

    private static string Str(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) ? el.GetString() ?? "" : "";

    private static int IntProp(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.TryGetInt32(out var v) ? v : 0;

    private static Dictionary<string, int> ReadIntMap(JsonElement obj, string name)
    {
        var map = new Dictionary<string, int>();
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
            foreach (var prop in el.EnumerateObject())
                if (prop.Value.TryGetInt32(out var v))
                    map[prop.Name] = v;
        return map;
    }

    private static string[] ReadStrings(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<string>();
        var list = new List<string>(arr.GetArrayLength());
        foreach (var item in arr.EnumerateArray())
            list.Add(item.GetString() ?? "");
        return list.ToArray();
    }
}
