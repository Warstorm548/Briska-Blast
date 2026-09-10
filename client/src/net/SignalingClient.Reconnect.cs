using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>The reconnect loop and the one terminal-close funnel. An unexpected
/// drop re-dials the same session WS for a bounded window — which is what makes
/// the server's host-reconnect grace reachable; an app-level rejection does not
/// retry at all.</summary>
public partial class SignalingClient
{
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
        Log.Info("net.signaling", $"connection lost (code {code}) — reconnecting…");
        Reconnecting?.Invoke();
    }

    private void ProcessReconnect(WebSocketPeer.State state)
    {
        switch (state)
        {
            case WebSocketPeer.State.Open:
                // Back up: re-identify on the fresh socket and resume.
                SendIdentifyOnce();
                _nextSyncMsec = 0; // re-sync now: the clock may have stepped while away
                _reconnecting = false;
                Log.Info("net.signaling", "reconnected.");
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
            Log.Warn("net.signaling", $"reconnect dial failed: {err}");
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
}
