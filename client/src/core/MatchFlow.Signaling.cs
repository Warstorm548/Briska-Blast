using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>The per-session signaling socket: created, subscribed, and torn down
/// in one matched pair. Every handler these two methods hook up lives in one of
/// the partials beside them.</summary>
public partial class MatchFlow
{
    // ---- the two sequences, converging in Preparing ----

    /// <summary>Create and connect the per-session signaling socket (a fresh
    /// node every session) and install this orchestrator as its sole
    /// lifecycle-event subscriber.</summary>
    private void OpenSignaling(SessionContext ctx)
    {
        var s = new SignalingClient();
        AddChild(s);
        s.Identified += OnIdentified;
        s.PeerJoined += OnPeerJoined;
        s.PeerLeft += OnPeerLeft;
        s.HostChanged += OnHostChanged;
        s.StartSignaling += OnStartSignaling;
        s.SessionEnded += OnSessionEnded;
        s.Kicked += OnKicked;
        s.Closed += OnClosed;
        s.GameOver += OnGameOver;
        s.MatchStarted += OnMatchStarted;
        s.Reconnected += OnReconnected;
        s.MatchPaused += OnMatchPaused;
        s.MatchResumed += OnMatchResumed;
        // Chat is recorded here, not by whichever view happens to be mounted.
        // The transcript has to span the lobby, Preparing and the match, and
        // during Preparing no view exists at all — a view-owned subscription is
        // exactly how lines used to be lost across the start.
        s.ChatMessage += OnChatMessage;
        s.ChatWarning += OnChatWarning;
        s.ChatBanned += OnChatBanned;
        s.ChatBodyDeleted += OnChatBodyDeleted;
        // Declare a rejoin identify only on the rejoin paths (BeginRejoin /
        // the poll recovery into an active match) — the server pauses the live
        // match for us. A lobby connect must never pause anything.
        s.IdentifyAsRejoin = IsRejoin;
        Signaling = s;
        s.Connect(ctx.SessionCode, ctx.PlayerId, ctx.SecretToken);
    }

    /// <summary>Unhook, close, and free the live signaling socket. Shared by
    /// the one teardown and the lobby poll's missed-start recovery (which
    /// replaces the socket with a fresh identify without ending the session).</summary>
    private void CloseSignaling(bool sendLeaveFrame)
    {
        if (Signaling is { } s)
        {
            s.Identified -= OnIdentified;
            s.PeerJoined -= OnPeerJoined;
            s.PeerLeft -= OnPeerLeft;
            s.HostChanged -= OnHostChanged;
            s.StartSignaling -= OnStartSignaling;
            s.SessionEnded -= OnSessionEnded;
            s.Kicked -= OnKicked;
            s.Closed -= OnClosed;
            s.GameOver -= OnGameOver;
            s.MatchStarted -= OnMatchStarted;
            s.Reconnected -= OnReconnected;
            s.MatchPaused -= OnMatchPaused;
            s.MatchResumed -= OnMatchResumed;
            s.ChatMessage -= OnChatMessage;
            s.ChatWarning -= OnChatWarning;
            s.ChatBanned -= OnChatBanned;
            s.ChatBodyDeleted -= OnChatBodyDeleted;
            if (sendLeaveFrame)
                s.SendLeave();
            s.CloseConnection();
            s.QueueFree();
        }
        Signaling = null;
    }
}
