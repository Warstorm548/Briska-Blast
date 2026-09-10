using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Game;

/// <summary>The connection-status overlay and the reconnect-grace signals that
/// drive it. Pure UI: the roster and host mutations behind these events belong to
/// <see cref="MatchFlow"/>.</summary>
public partial class GameScene
{
    // ---- host-loss grace UI (Stage 4) ----

    // Reconnect grace overlay. A client shows at most one message at a time:
    // its own socket dropped (self), the host's (host), or a peer's (peer).
    private CanvasLayer _overlayLayer = null!;
    private Label _overlay = null!;
    private bool _selfReconnecting;
    private bool _hostReconnecting;
    private bool _peerReconnecting;
    private ulong _peerReconnectHideMsec;

    // Always-visible session code so players can reshare it with a friend who
    // dropped and needs to re-enter it to rejoin the match.
    private Label _codeLabel = null!;

    private void OnHostChangedInGame(string playerId)
    {
        // Promotion landed (or a voluntary transfer): clear the "host
        // reconnecting…" overlay. The roster/host mutation itself is
        // MatchFlow's — this handler is pure UI.
        _hostReconnecting = false;
        UpdateOverlay();
    }

    /// <summary>The host dropped. Raise the overlay hint; the mesh holds until they
    /// return or their grace expires.</summary>
    private void OnHostReconnecting(string playerId, int graceSecs)
    {
        _hostReconnecting = true;
        UpdateOverlay();
    }

    /// <summary>The host is back. Clear the hint.</summary>
    private void OnHostReconnected(string playerId)
    {
        _hostReconnecting = false;
        UpdateOverlay();
    }

    /// <summary>A non-host peer dropped mid-game. Flags the reconnect window on the
    /// overlay; play continues over the rest of the mesh.</summary>
    private void OnPeerReconnecting(string playerId, int graceSecs)
    {
        // A non-host peer dropped mid-game. Show a brief hint; their slot is held
        // longer (for a manual rejoin), but the overlay only flags the window —
        // auto-hide after graceSecs (checked in _PhysicsProcess), or sooner if
        // the mesh heals. The ball keeps flowing over the rest of the mesh.
        _peerReconnecting = true;
        _peerReconnectHideMsec = Time.GetTicksMsec() + (ulong)Mathf.Max(graceSecs, 0) * 1000UL;
        UpdateOverlay();
    }

    /// <summary>This client lost its own connection. Raise the hint — the match is
    /// still running on the other screens.</summary>
    private void OnSelfReconnecting()
    {
        _selfReconnecting = true;
        UpdateOverlay();
    }

    /// <summary>This client is back on. Clear the hint.</summary>
    private void OnSelfReconnected()
    {
        _selfReconnecting = false;
        UpdateOverlay();
    }

    /// <summary>Build the connection-status overlay: one centred Label on a
    /// high-layer <see cref="CanvasLayer"/> so it floats above the field and every
    /// other HUD element. Created hidden — <see cref="UpdateOverlay"/> decides when
    /// it has something to say.</summary>
    private void BuildOverlay()
    {
        _overlayLayer = new CanvasLayer { Layer = 100 };
        AddChild(_overlayLayer);
        _overlay = new Label
        {
            Visible = false,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };
        _overlay.SetAnchorsPreset(Control.LayoutPreset.FullRect);
        _overlay.AddThemeFontSizeOverride("font_size", 64);
        _overlayLayer.AddChild(_overlay);

        // Session code, top-RIGHT, so a player can read it back to a dropped friend
        // who needs to re-enter it on the Join screen to rejoin. It sat top-left
        // until 0.34.0, overlapping the scoreboard that lived there; the
        // leaderboard now owns that corner, and the pause menu carries the code
        // with a Copy button anyway, so this is the convenience copy.
        var code = SessionContext.Instance?.SessionCode ?? "";
        _codeLabel = new Label { Text = $"Code: {code}" };
        // Spans the top and right-ALIGNS its text rather than being a right-anchored
        // box: a Label's width comes from its own text, so anchoring the box to the
        // right edge and nudging it would run a longer code off screen.
        _codeLabel.SetAnchorsPreset(Control.LayoutPreset.TopWide);
        _codeLabel.HorizontalAlignment = HorizontalAlignment.Right;
        _codeLabel.OffsetLeft = 0;
        _codeLabel.OffsetRight = -16;
        _codeLabel.OffsetTop = 12;
        _codeLabel.OffsetBottom = 48;
        _codeLabel.AddThemeFontSizeOverride("font_size", 24);
        _overlayLayer.AddChild(_codeLabel);
    }

    /// <summary>Repaint the connection overlay from the reconnect flags, in
    /// priority order: this client's own drop outranks the host's, which outranks
    /// another player's. No flag set hides it. Every flag change routes through
    /// here, so the precedence lives in exactly one place.</summary>
    private void UpdateOverlay()
    {
        string msg =
            _selfReconnecting ? "Reconnecting…" :
            _hostReconnecting ? "Host reconnecting…" :
            _peerReconnecting ? "A player is reconnecting…" :
            "";
        _overlay.Text = msg;
        _overlay.Visible = msg.Length > 0;
    }
}
