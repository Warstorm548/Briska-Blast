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
/// Stage 1 (lobby) uses the lifecycle frames: it sends <c>identify</c> on
/// open and <c>leave</c> on request, and surfaces Identified / PeerJoined /
/// PeerLeft / HostChanged / StartSignaling / SessionEnded / Kicked / Closed.
/// The WebRTC frames (offer/answer/ice_candidate) are received but ignored
/// here — peer negotiation is a later stage.
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
    /// <summary>Socket closed. Carries the close code (4xxx app codes from
    /// the server, or transport codes like 1006) and the reason string.</summary>
    public event Action<int, string>? Closed;

    // WebRTC negotiation relays (server attests `from`). The transport layer
    // subscribes to these and feeds them into the matching peer connection.
    public event Action<string, string>? OfferReceived;          // (from, sdp)
    public event Action<string, string>? AnswerReceived;         // (from, sdp)
    public event Action<string, string, string, int>? IceCandidateReceived; // (from, candidate, sdpMid, sdpMLineIndex)

    private readonly WebSocketPeer _ws = new();
    private string _playerId = "";
    private string _secretToken = "";
    private bool _active;
    private bool _identifySent;
    private bool _closedEmitted;

    private sealed record IdentifyFrame(string Type, string PlayerId, string SecretToken);
    private sealed record LeaveFrame(string Type);
    private sealed record OfferFrame(string Type, string To, string Sdp);
    private sealed record AnswerFrame(string Type, string To, string Sdp);
    private sealed record IceFrame(string Type, string To, string Candidate, string SdpMid, int SdpMLineIndex);
    private sealed record PeerConnectionFailedFrame(string Type, string Peer, string Reason);

    /// <summary>Open the WS for <paramref name="code"/> and begin identifying.</summary>
    public void Connect(string code, string playerId, string secretToken)
    {
        _playerId = playerId;
        _secretToken = secretToken;

        // Reset lifecycle flags so a reused instance re-sends identify and can
        // emit Closed again, rather than carrying stale state from a prior run.
        _identifySent = false;
        _closedEmitted = false;
        _active = false;

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

        switch (_ws.GetReadyState())
        {
            case WebSocketPeer.State.Open:
                if (!_identifySent)
                {
                    _ws.SendText(JsonSerializer.Serialize(
                        new IdentifyFrame("identify", _playerId, _secretToken), Json.Options));
                    _identifySent = true;
                }
                while (_ws.GetAvailablePacketCount() > 0)
                    HandleFrame(Encoding.UTF8.GetString(_ws.GetPacket()));
                break;

            case WebSocketPeer.State.Closed:
                EmitClosedOnce(_ws.GetCloseCode(), _ws.GetCloseReason());
                break;
        }
    }

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
