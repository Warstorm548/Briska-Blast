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

    private readonly WebSocketPeer _ws = new();
    private string _playerId = "";
    private string _secretToken = "";
    private bool _active;
    private bool _identifySent;
    private bool _closedEmitted;

    private sealed record IdentifyFrame(string Type, string PlayerId, string SecretToken);
    private sealed record LeaveFrame(string Type);

    /// <summary>Open the WS for <paramref name="code"/> and begin identifying.</summary>
    public void Connect(string code, string playerId, string secretToken)
    {
        _playerId = playerId;
        _secretToken = secretToken;

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
        if (_ws.GetReadyState() == WebSocketPeer.State.Open)
            _ws.SendText(JsonSerializer.Serialize(new LeaveFrame("leave"), Json.Options));
    }

    /// <summary>Initiate a clean close. Safe to call repeatedly.</summary>
    public void CloseConnection()
    {
        if (_active)
            _ws.Close();
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
                case "answer":
                case "ice_candidate":
                    // WebRTC negotiation is a later stage — acknowledged, not acted on.
                    GD.Print($"[signaling] received '{typeEl.GetString()}' (WebRTC stage handles this)");
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
