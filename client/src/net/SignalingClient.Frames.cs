namespace BriskaBlast.Net;

/// <summary>The outgoing wire format: one record per frame this client sends, and
/// the typed senders that put them on the socket. Everything here funnels through
/// <c>SendFrame</c>, which drops the frame when the socket isn't open.</summary>
public partial class SignalingClient
{
    private sealed record IdentifyFrame(string Type, string PlayerId, string SecretToken, bool Rejoin);
    private sealed record LeaveFrame(string Type);
    private sealed record OfferFrame(string Type, string To, string Sdp);
    private sealed record AnswerFrame(string Type, string To, string Sdp);
    private sealed record IceFrame(string Type, string To, string Candidate, string SdpMid, int SdpMLineIndex);
    private sealed record PeerConnectionFailedFrame(string Type, string Peer, string Reason);
    private sealed record ReportScoreFrame(string Type, string ScoringPlayerId, int Points);
    private sealed record TimeSyncFrame(string Type, long ClientSendMs);
    private sealed record SendChatFrame(string Type, string Text);
    private sealed record ClientReadyFrame(string Type);

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
    public void SendReportScore(string scoringPlayerId, int points) =>
        SendFrame(new ReportScoreFrame("report_score", scoringPlayerId, points));

    /// <summary>Send a lobby chat message. The server attests the sender,
    /// resolves the display name, and broadcasts <see cref="ChatMessage"/> to the
    /// whole room (including this client). No-op if the socket isn't open.</summary>
    public void SendChatMessage(string text) =>
        SendFrame(new SendChatFrame("send_chat", text));

    /// <summary>Report that this client's WebRTC mesh is fully up — its half of
    /// the ready barrier. The server answers with <see cref="MatchStarted"/>
    /// once everyone is ready (or immediately if the match already started).
    /// Safe to re-send (e.g. after a WS reconnect); the server treats a
    /// duplicate as a straggler and replies directly.</summary>
    public void SendClientReady() =>
        SendFrame(new ClientReadyFrame("client_ready"));
}
