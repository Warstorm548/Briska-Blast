using System;
using System.Collections.Generic;

namespace BriskaBlast.Net;

/// <summary>
/// Topology-agnostic peer transport the game layer (Stage 3) talks to instead
/// of WebRTC directly. The mesh-vs-relay-vs-SFU decision becomes a per-game-mode
/// choice of <see cref="IPeerTransport"/> implementation, so adding a new
/// topology later is additive — game logic never names WebRTC or "mesh".
/// See docs/planning/multiplayer-client-stages.md.
/// </summary>
public interface IPeerTransport
{
    /// <summary>A peer's channel is open and ready to carry data.</summary>
    event Action<string> PeerConnected;
    /// <summary>Bytes arrived from a peer.</summary>
    event Action<string, byte[]> PeerData;
    /// <summary>A previously-connected peer's link closed.</summary>
    event Action<string> PeerDisconnected;
    /// <summary>A peer's connection could not be established (e.g. ICE failed).</summary>
    event Action<string> PeerFailed;

    /// <summary>Begin connecting this player to every other peer in the session.</summary>
    void Connect(string selfId, IReadOnlyList<string> peerIds);
    /// <summary>Tear down any stale link to <paramref name="peerId"/> and
    /// re-negotiate from scratch. Used when a peer that dropped mid-game (its
    /// link died) rejoins, so both sides rebuild that one connection without
    /// disturbing the rest of the mesh.</summary>
    void ResyncPeer(string peerId);
    void Send(string peerId, byte[] data);
    void Broadcast(byte[] data);
    void Close();
}
