using Godot;
using System.Collections.Generic;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>Preparing's mesh bring-up: create the transport, count data channels
/// against the expected roster, and report <c>client_ready</c> once every peer is
/// in. The deadline these fields feed is enforced by <c>_Process</c>.</summary>
public partial class MatchFlow
{
    /// <summary>How long Preparing may take (signaling identify on a rejoin +
    /// full mesh bring-up + the server's ready barrier) before the attempt
    /// fails back to the main menu. Sized to the signaling reconnect window /
    /// server promotion grace, with room for TURN-relay ICE; the server's own
    /// barrier valve (20s) always resolves under this deadline, so waiting on
    /// <c>match_started</c> can only time out if the server is unreachable.</summary>
    private const ulong PrepareTimeoutMsec = 30_000;

    // Preparing bookkeeping: the mesh is "up" when every expected peer's data
    // channel has opened. The transport has no aggregate signal — we count its
    // per-peer events against the roster we handed it.
    private readonly HashSet<string> _expectedPeers = new();
    private readonly HashSet<string> _connectedPeers = new();
    private readonly HashSet<string> _failedPeers = new();
    private ulong _prepareDeadlineMsec;

    // True once this client's mesh completed and its `client_ready` went out.
    // From then on Preparing waits solely on the server's `match_started` —
    // the only door into InMatch — and a WS blip re-sends the ready (it may
    // have been lost mid-reconnect; the server answers a duplicate directly).
    private bool _readySent;

    /// <summary>Shared mesh bring-up — the single successor to the duplicated
    /// lobby-start / rejoin choreographies. Creates the transport, applies the
    /// match's ICE servers (must precede Connect — the config is baked into
    /// each peer connection at creation), and starts counting channels.</summary>
    private void StartMesh(SessionContext ctx, IReadOnlyList<string> expectedPeers,
        IceServerDto[] iceServers)
    {
        _expectedPeers.Clear();
        _connectedPeers.Clear();
        _failedPeers.Clear();
        _readySent = false;
        foreach (var p in expectedPeers)
            if (p != ctx.PlayerId)
                _expectedPeers.Add(p);

        var transport = new WebRtcMeshTransport();
        transport.Init(Signaling!);
        transport.SetIceServers(iceServers);
        AddChild(transport);
        transport.PeerConnected += OnMeshPeerConnected;
        transport.PeerFailed += OnMeshPeerFailed;
        transport.PeerDisconnected += OnMeshPeerDisconnected;
        Transport = transport;
        transport.Connect(ctx.PlayerId, new List<string>(_expectedPeers));

        EmitPreparing($"Connecting to players (0/{_expectedPeers.Count})…");
        CheckPreparingComplete(); // zero expected peers → straight in
    }

    private void OnMeshPeerConnected(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _connectedPeers.Add(peerId);
        _failedPeers.Remove(peerId);
        int have = 0;
        foreach (var p in _expectedPeers)
            if (_connectedPeers.Contains(p))
                have++;
        EmitPreparing($"Connecting to players ({have}/{_expectedPeers.Count})…");
        CheckPreparingComplete();
    }

    private void OnMeshPeerFailed(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _failedPeers.Add(peerId);
        _connectedPeers.Remove(peerId);
    }

    private void OnMeshPeerDisconnected(string peerId)
    {
        if (State != MatchFlowState.Preparing)
            return;
        _connectedPeers.Remove(peerId);
    }

    private void CheckPreparingComplete()
    {
        if (State != MatchFlowState.Preparing)
            return;
        if (!_expectedPeers.IsSubsetOf(_connectedPeers))
            return;

        // Mesh up ≠ match on: everyone else's mesh must be up too. Report
        // ready and hold for the server's match_started (broadcast when all
        // are ready or its 20s valve fires — always under our deadline).
        if (_readySent)
            return;
        _readySent = true;
        Log.Info("match.flow", "mesh complete — client_ready sent, waiting for match_started.");
        Signaling?.SendClientReady();
        EmitPreparing("Waiting for other players…");
    }
}
