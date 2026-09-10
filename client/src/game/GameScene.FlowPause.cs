using Godot;
using BriskaBlast.UI.Menus;

namespace BriskaBlast.Game;

/// <summary>The server-driven pause-on-rejoin freeze: every screen stops together
/// while a player reconnects, then unfreezes on a shared countdown. Distinct from
/// the local Esc menu in GameScene.PauseMenu.cs.</summary>
public partial class GameScene
{
    // ---- pause-on-rejoin (MatchFlow's match_paused/match_resumed relays) ----

    // Pause-on-rejoin freeze (server match_paused/match_resumed): while set,
    // _PhysicsProcess steps nothing — every screen freezes together so balls
    // aren't sent at the rejoiner's still-walled edge. Mirrors the _gameOver
    // latch; resolved by the resume countdown reaching zero. The PreparingPanel
    // is reused as the overlay ("Waiting for {name}…", no Cancel — mid-match
    // there is nothing local to cancel).
    private PreparingPanel? _pausePanel;
    private bool _flowPaused;
    private bool _resumeCountdownActive;
    private ulong _resumeAtMsec;

    private void OnMatchPaused(string displayName)
    {
        if (_gameOver)
            return;
        _flowPaused = true;
        // A second rejoiner while already paused just updates the name; a pause
        // landing mid-countdown cancels the countdown (a new hold arrived).
        _resumeCountdownActive = false;

        if (_pausePanel == null)
        {
            _pausePanel = GD.Load<PackedScene>("res://src/ui/menus/PreparingPanel.tscn")
                .Instantiate<PreparingPanel>();
            _overlayLayer.AddChild(_pausePanel);
            _pausePanel.SetAnchorsPreset(Control.LayoutPreset.Center);
            _pausePanel.ShowCancel(false);
        }
        _pausePanel.SetTitle("Match paused");
        _pausePanel.SetStatus($"Waiting for {displayName} to reconnect…");
    }

    /// <summary>The server says the frozen match is restarting in
    /// <paramref name="countdownSecs"/>. Arms the countdown that
    /// <see cref="TickFlowPause"/> paints and unfreezes on. Ignored once the match
    /// is over, or if this screen was never frozen.</summary>
    private void OnMatchResumed(int countdownSecs)
    {
        if (_gameOver || !_flowPaused)
            return;
        _resumeCountdownActive = true;
        _resumeAtMsec = Time.GetTicksMsec() + (ulong)Mathf.Max(countdownSecs, 0) * 1000UL;
    }

    /// <summary>Tear down the "waiting for a player" panel, if one is up. Safe to
    /// call when there is none, so every exit path can call it unconditionally.</summary>
    private void RemovePausePanel()
    {
        _pausePanel?.QueueFree();
        _pausePanel = null;
    }

    /// <summary>Drive the frozen phase each physics tick: paint the resume
    /// countdown once it's running and unfreeze when it reaches zero. Returns
    /// true while the sim must stay frozen.</summary>
    private bool TickFlowPause()
    {
        if (!_flowPaused)
            return false;
        if (!_resumeCountdownActive)
            return true; // waiting on the rejoiner / the server valve

        ulong now = Time.GetTicksMsec();
        if (now < _resumeAtMsec)
        {
            int remaining = (int)((_resumeAtMsec - now + 999) / 1000);
            _pausePanel?.SetTitle("Match resuming");
            _pausePanel?.SetStatus($"Resuming in {remaining}…");
            return true;
        }

        _flowPaused = false;
        _resumeCountdownActive = false;
        RemovePausePanel();
        return false;
    }
}
