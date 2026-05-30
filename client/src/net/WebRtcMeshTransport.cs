using System;
using System.Collections.Generic;
using Godot;

namespace BriskaBlast.Net;

/// <summary>
/// Full-mesh <see cref="IPeerTransport"/> over WebRTC: one
/// <see cref="WebRtcPeerConnection"/> + one reliable <see cref="WebRtcDataChannel"/>
/// per peer. A <see cref="Node"/> so it can poll every connection each frame.
///
/// Negotiation rides the existing <see cref="SignalingClient"/> (offer/answer/ice
/// relayed by the server). Glare is avoided by a deterministic rule: in each
/// pair the lexicographically-smaller player_id is the offerer (and creates the
/// data channel); the other answers. STUN only (no TURN yet) — symmetric-NAT
/// peers will fail until the TURN work lands.
/// </summary>
public partial class WebRtcMeshTransport : Node, IPeerTransport
{
    public event Action<string>? PeerConnected;
    public event Action<string, byte[]>? PeerData;
    public event Action<string>? PeerDisconnected;
    public event Action<string>? PeerFailed;

    private static readonly Godot.Collections.Dictionary IceConfig = new()
    {
        {
            "iceServers", new Godot.Collections.Array
            {
                new Godot.Collections.Dictionary
                {
                    { "urls", new Godot.Collections.Array { "stun:stun.l.google.com:19302" } },
                },
            }
        },
    };

    private sealed class PeerLink
    {
        public WebRtcPeerConnection Pc = null!;
        public WebRtcDataChannel? Channel;
        public bool RemoteSet;
        public bool Connected;
        public bool Failed;
        public bool Disconnected;
        public readonly List<(string Mid, int Index, string Candidate)> PendingIce = new();
    }

    private SignalingClient _signaling = null!;
    private string _selfId = "";
    private readonly Dictionary<string, PeerLink> _peers = new();

    /// <summary>Wire to the lobby's signaling connection. Call before Connect.</summary>
    public void Init(SignalingClient signaling)
    {
        _signaling = signaling;
        _signaling.OfferReceived += OnOfferReceived;
        _signaling.AnswerReceived += OnAnswerReceived;
        _signaling.IceCandidateReceived += OnIceReceived;
    }

    public void Connect(string selfId, IReadOnlyList<string> peerIds)
    {
        _selfId = selfId;
        foreach (var pid in peerIds)
        {
            if (pid == selfId || _peers.ContainsKey(pid))
                continue;

            var link = NewLink(pid);
            _peers[pid] = link;

            // Deterministic offerer: smaller id offers + owns the data channel.
            if (string.CompareOrdinal(selfId, pid) < 0)
            {
                link.Channel = link.Pc.CreateDataChannel("data");
                link.Pc.CreateOffer();
            }
            // Otherwise we answer once the offer arrives (OnOfferReceived).
        }
    }

    public void ResyncPeer(string peerId)
    {
        if (peerId == _selfId)
            return;

        // Drop any stale link (a dead WebRTC connection from before the peer
        // dropped) so negotiation restarts cleanly.
        if (_peers.TryGetValue(peerId, out var old))
        {
            old.Channel?.Close();
            old.Pc.Close();
            _peers.Remove(peerId);
        }

        var link = NewLink(peerId);
        _peers[peerId] = link;

        // Same deterministic-offerer rule as Connect: the smaller id offers and
        // owns the data channel; the other answers when the offer arrives.
        if (string.CompareOrdinal(_selfId, peerId) < 0)
        {
            link.Channel = link.Pc.CreateDataChannel("data");
            link.Pc.CreateOffer();
        }
    }

    public void Send(string peerId, byte[] data)
    {
        if (_peers.TryGetValue(peerId, out var link) &&
            link.Channel is { } ch && ch.GetReadyState() == WebRtcDataChannel.ChannelState.Open)
        {
            ch.PutPacket(data);
        }
    }

    public void Broadcast(byte[] data)
    {
        foreach (var pid in _peers.Keys)
            Send(pid, data);
    }

    public void Close()
    {
        // Detach signaling handlers so a reused instance (or a late teardown)
        // can't double-subscribe or fire callbacks after close.
        if (_signaling is not null)
        {
            _signaling.OfferReceived -= OnOfferReceived;
            _signaling.AnswerReceived -= OnAnswerReceived;
            _signaling.IceCandidateReceived -= OnIceReceived;
            _signaling = null!;
        }
        foreach (var link in _peers.Values)
        {
            link.Channel?.Close();
            link.Pc.Close();
        }
        _peers.Clear();
    }

    private PeerLink NewLink(string pid)
    {
        var pc = new WebRtcPeerConnection();
        if (pc.Initialize(IceConfig) != Error.Ok)
            GD.PushWarning($"[webrtc] Initialize failed for peer {pid} — is the webrtc-native extension installed?");

        var link = new PeerLink { Pc = pc };

        pc.SessionDescriptionCreated += (type, sdp) => OnLocalDescription(pid, type, sdp);
        pc.IceCandidateCreated += (media, index, name) => _signaling.SendIceCandidate(pid, name, media, (int)index);
        pc.DataChannelReceived += channel => link.Channel = channel;

        return link;
    }

    private void OnLocalDescription(string pid, string type, string sdp)
    {
        if (!_peers.TryGetValue(pid, out var link))
            return;
        link.Pc.SetLocalDescription(type, sdp);
        if (type == "offer")
            _signaling.SendOffer(pid, sdp);
        else
            _signaling.SendAnswer(pid, sdp);
    }

    private void OnOfferReceived(string from, string sdp)
    {
        if (!_peers.TryGetValue(from, out var link))
            return;
        // SetRemoteDescription(offer) triggers SessionDescriptionCreated(answer).
        link.Pc.SetRemoteDescription("offer", sdp);
        link.RemoteSet = true;
        FlushPendingIce(link);
    }

    private void OnAnswerReceived(string from, string sdp)
    {
        if (!_peers.TryGetValue(from, out var link))
            return;
        link.Pc.SetRemoteDescription("answer", sdp);
        link.RemoteSet = true;
        FlushPendingIce(link);
    }

    private void OnIceReceived(string from, string candidate, string sdpMid, int sdpMLineIndex)
    {
        if (!_peers.TryGetValue(from, out var link))
            return;
        // ICE can arrive before the remote description is set; buffer until then.
        if (link.RemoteSet)
            link.Pc.AddIceCandidate(sdpMid, sdpMLineIndex, candidate);
        else
            link.PendingIce.Add((sdpMid, sdpMLineIndex, candidate));
    }

    private static void FlushPendingIce(PeerLink link)
    {
        foreach (var (mid, index, cand) in link.PendingIce)
            link.Pc.AddIceCandidate(mid, index, cand);
        link.PendingIce.Clear();
    }

    public override void _Process(double delta)
    {
        // Snapshot: event handlers below may mutate scene state; don't iterate
        // the live dictionary.
        foreach (var (pid, link) in new List<KeyValuePair<string, PeerLink>>(_peers))
        {
            link.Pc.Poll();

            if (!link.Failed && link.Pc.GetConnectionState() == WebRtcPeerConnection.ConnectionState.Failed)
            {
                link.Failed = true;
                _signaling.SendPeerConnectionFailed(pid, "ice_failed");
                PeerFailed?.Invoke(pid);
            }

            if (link.Channel is not { } ch)
                continue;

            ch.Poll();
            var state = ch.GetReadyState();

            if (!link.Connected && state == WebRtcDataChannel.ChannelState.Open)
            {
                link.Connected = true;
                PeerConnected?.Invoke(pid);
            }

            while (ch.GetAvailablePacketCount() > 0)
                PeerData?.Invoke(pid, ch.GetPacket());

            if (link.Connected && !link.Disconnected && state == WebRtcDataChannel.ChannelState.Closed)
            {
                link.Disconnected = true;
                PeerDisconnected?.Invoke(pid);
            }
        }
    }
}
