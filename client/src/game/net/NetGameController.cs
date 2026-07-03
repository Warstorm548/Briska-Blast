using System;
using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.Game.Net;
using BriskaBlast.Net;

namespace BriskaBlast.Game;

/// <summary>
/// Sits between the local simulation, the WebRTC mesh transport, and the
/// session signaling socket. Takes sim events out to the network (handoffs +
/// score reports), brings network events into the simulation (inbound balls +
/// authoritative ScoreUpdate), and keeps the edge map honest as peers come and
/// go. Names only <see cref="IPeerTransport"/> — never WebRTC.
/// </summary>
public sealed class NetGameController : IDisposable
{
    private readonly GameState _state;
    private readonly IPeerTransport _transport;
    private readonly SignalingClient _signaling;
    private readonly float _spawnRadius;

    // peerId → local edge assigned to that peer. Built from GameState.Edges at
    // construction; an entry is removed when its peer drops (and the edge is
    // flipped to a wall) so an inbound packet from a gone peer is dropped.
    private readonly Dictionary<string, Edge> _peerToEdge = new();

    // peerId → its ORIGINAL local edge, captured once and never pruned. Lets a
    // rejoining peer's walled edge be restored to the same portal it held before
    // it dropped (OnPeerLost removes the live mapping above, losing it otherwise).
    private readonly Dictionary<string, Edge> _peerHomeEdge = new();

    public NetGameController(GameState state, IPeerTransport transport,
        SignalingClient signaling, float spawnRadius)
    {
        _state = state;
        _transport = transport;
        _signaling = signaling;
        _spawnRadius = spawnRadius;

        foreach (var kv in _state.Edges)
            if (kv.Value.Kind == EdgeKind.Portal)
            {
                _peerToEdge[kv.Value.PeerId] = kv.Key;
                _peerHomeEdge[kv.Value.PeerId] = kv.Key;
            }

        _transport.PeerData += OnPeerData;
        _transport.PeerDisconnected += OnPeerLost;
        _transport.PeerFailed += OnPeerLost;
        _signaling.ScoreUpdate += OnScoreUpdate;
        _signaling.PeerJoined += OnPeerRejoined;
    }

    /// <summary>Push one sim handoff out to its peer (directed Send).</summary>
    public void SendHandoff(HandoffEvent ev)
    {
        var (perp, tang) = BallTransform.ToCanonical(ev.ExitEdge, ev.Velocity);
        // Speed crosses the wire as a fraction of arena HEIGHT per second so it
        // means the same on a differently-sized peer screen. Dividing BOTH
        // components by the same reference (height) keeps tang/perp — the entry
        // angle — invariant across aspect ratios; the receiver multiplies back by
        // ITS own height. ArenaHeight comes from the viewport so it's > 0.
        float invH = 1f / _state.ArenaHeight;
        var pkt = new BallHandoffPacket(
            ev.BallId, ev.LastHitterId, perp * invH, tang * invH, ev.NormalizedAlong,
            _signaling.ServerNowMs(), ev.Kind);
        bool sent = _transport.Send(ev.PeerId, GamePacket.WriteBallHandoff(pkt));
        if (sent)
            Log.Debug("game.handoff", $"OUT ball={ev.BallId} kind={ev.Kind} peer={ev.PeerId} edge={ev.ExitEdge}");
        else
            // The ball just left our screen but the peer channel isn't open, so it
            // vanished into nothing — the classic "no balls crossing" symptom.
            Log.Warn("game.handoff", $"DROPPED ball={ev.BallId} peer={ev.PeerId} — channel not open (ball lost)");
    }

    /// <summary>Report a credited score to the server (server-relayed channel).
    /// Empty <see cref="ScoreEvent.ScoringPlayerId"/> is a self-goal or untouched
    /// ball — no point is awarded, so nothing is sent (the local serve still
    /// fires regardless, handled by GameScene).</summary>
    public void ReportScore(ScoreEvent ev)
    {
        if (!string.IsNullOrEmpty(ev.ScoringPlayerId))
            _signaling.SendReportScore(ev.ScoringPlayerId, ev.Points);
    }

    private void OnPeerData(string peerId, byte[] data)
    {
        if (!GamePacket.TryReadBallHandoff(data, out var pkt))
            return;
        if (!_peerToEdge.TryGetValue(peerId, out var entryEdge))
            return; // peer no longer mapped (already dropped) — discard

        // The numbers came off the wire from a peer: reject NaN/Infinity so a
        // malformed or hostile packet can't poison the ball's pos/vel (a NaN
        // ball never collides and corrupts the sim). Along is a fraction; clamp it.
        if (!float.IsFinite(pkt.Perp) || !float.IsFinite(pkt.Tang) || !float.IsFinite(pkt.Along))
            return;
        float along = Mathf.Clamp(pkt.Along, 0f, 1f);

        // Perp/Tang arrived as a fraction of arena HEIGHT per second; scale by
        // THIS client's height to recover pixels/second in our own frame, so the
        // ball enters at the same relative pace/angle regardless of the sender's
        // screen size. The transit fast-forward below is then in our pixels too.
        float h = _state.ArenaHeight;
        var (pos, vel) = BallTransform.FromCanonical(
            entryEdge, pkt.Perp * h, pkt.Tang * h, along,
            _state.ArenaWidth, _state.ArenaHeight, _spawnRadius);

        // Fast-forward by transit time so the ball doesn't visually lag at entry.
        // Both ends timestamp in the server-synced frame (ServerClock), so this
        // measures real network delay — not the gap between two machines' wall
        // clocks, which used to drift apart and fling the ball partway down the
        // screen. Until our clock has a sample, skip the correction and enter
        // cleanly at the edge; the clamp is a safety net against a bad sample.
        float transit = _signaling.ClockSynced
            ? Mathf.Clamp((_signaling.ServerNowMs() - pkt.SentTimestampMs) / 1000f, 0f, 0.5f)
            : 0f;
        pos += vel * transit;

        // Defence: drop a ball we already have (a packet replay, or a ball that
        // crossed back). Ball ids are globally unique (seat-namespaced), so this
        // scan is correct with multiple balls in play.
        foreach (var b in _state.Balls)
            if (b.Id == pkt.BallId)
                return;

        _state.Balls.Add(new Ball
        {
            Id = pkt.BallId,
            Pos = pos,
            Vel = vel,
            Radius = _spawnRadius,
            LastHitterId = pkt.LastHitterId,
            Kind = pkt.Kind,
        });
        Log.Debug("game.handoff", $"IN  ball={pkt.BallId} kind={pkt.Kind} peer={peerId} edge={entryEdge}");
    }

    private void OnPeerLost(string peerId)
    {
        // Flip the lost peer's portal edge to a solid wall so any future ball
        // heading at them bounces instead of vanishing into a dead channel.
        if (_peerToEdge.TryGetValue(peerId, out var edge))
        {
            Log.Info("session", $"peer={peerId} lost — edge {edge} → wall");
            _state.Edges[edge] = EdgeTarget.Wall;
            _peerToEdge.Remove(peerId);
        }
    }

    /// <summary>A peer (re)identified in the session. If it's one of our home
    /// peers whose link we'd walled off (it dropped mid-game and is rejoining),
    /// restore its portal edge and re-negotiate just that connection. Ignored
    /// when the peer is still live (a process-alive WS blip — its mesh link
    /// never died, so it stays in <see cref="_peerToEdge"/>) or isn't ours.
    /// The peer's username (second arg) is irrelevant to mesh healing — the
    /// lobby/scoreboard consume it via SessionContext.</summary>
    private void OnPeerRejoined(string peerId, string username)
    {
        _ = username; // not used for re-meshing; carried by the PeerJoined event
        if (!_peerHomeEdge.TryGetValue(peerId, out var edge))
            return; // not one of this screen's portal peers
        if (_peerToEdge.ContainsKey(peerId))
            return; // link never lost — nothing to heal

        Log.Info("session", $"peer={peerId} rejoined — restoring edge {edge} → portal, re-meshing");
        _state.Edges[edge] = EdgeTarget.Portal(peerId);
        _peerToEdge[peerId] = edge;
        _transport.ResyncPeer(peerId);
    }

    private void OnScoreUpdate(Dictionary<string, int> scores)
    {
        // Server is authoritative — overwrite, never add. A dropped/duplicated
        // ScoreUpdate can't desync the local tally.
        _state.Scores.Clear();
        foreach (var (pid, pts) in scores)
            _state.Scores[pid] = pts;
    }

    public void Dispose()
    {
        _transport.PeerData -= OnPeerData;
        _transport.PeerDisconnected -= OnPeerLost;
        _transport.PeerFailed -= OnPeerLost;
        _signaling.ScoreUpdate -= OnScoreUpdate;
        _signaling.PeerJoined -= OnPeerRejoined;
    }
}
