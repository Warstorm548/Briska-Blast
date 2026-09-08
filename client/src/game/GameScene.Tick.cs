using Godot;

namespace BriskaBlast.Game;

/// <summary>The per-frame match tick: the pause/over gates, input polling and
/// the ordering of simulation, handoff, scoring and repaint. The timers it
/// drives and the events it dispatches live in the partials beside it.</summary>
public partial class GameScene
{
    /// <summary>Input action per hotbar slot, indexed by slot. Held as a table because
    /// _PhysicsProcess polls all of them every frame and building the names inline would
    /// allocate a string per slot per frame. This is also the authority on which slots
    /// are reachable: growing <see cref="Hotbar.SlotCount"/> without adding matching
    /// actions here (and in project.godot) just leaves the extra slots keyless.</summary>
    private static readonly string[] HotbarActions =
    {
        "hotbar_slot_1", "hotbar_slot_2", "hotbar_slot_3", "hotbar_slot_4", "hotbar_slot_5",
    };

    /// <summary>The match tick: step the simulation, poll input, and run the
    /// per-frame timers. Returns early and drives nothing once the match is over or
    /// while the flow is frozen for a rejoin, so those two states are the only
    /// things that can stop the sim.</summary>
    public override void _PhysicsProcess(double delta)
    {
        // Match over: the simulation is frozen behind the end screen. Step nothing,
        // accept no input — the last paint in OnGameOver stays on screen.
        if (_gameOver)
            return;

        // Paused for a rejoiner: every screen freezes together (input, spawns,
        // sim, handoffs) until the resume countdown runs out.
        if (TickFlowPause())
            return;

        var dt = (float)delta;

        // Auto-hide the "a player is reconnecting…" hint once its window elapses.
        if (_peerReconnecting && Time.GetTicksMsec() >= _peerReconnectHideMsec)
        {
            _peerReconnecting = false;
            UpdateOverlay();
        }

        // Escape toggles the in-match pause menu (open ⇄ Return to Session) —
        // unless chat holds the keyboard, where it is the way out of the input
        // instead. Escape IS consumed by an editing LineEdit, but only to leave
        // edit mode, and that preserves focus (Godot 4.4+ keeps the two apart) —
        // so without this branch chat would sit there holding the latch with no
        // caret, and the paddle would never come back. Polling sidesteps the
        // consumption either way: this reads the raw action, not the event.
        if (Input.IsActionJustPressed("ui_cancel"))
        {
            if (_chatFocused)
                _chat.ReleaseInput();
            else
                TogglePauseMenu();
        }

        // Hotbar: number keys 1-5 fire their own slot. Suspended while the pause menu
        // is open or chat holds the keyboard, like the paddle and the serve; the
        // _gameOver / flow-pause returns above already cover the end screen and a
        // rejoin freeze. Deliberately live during the pre-serve wait — using an item
        // before serving is harmless.
        if (!_paused && !_chatFocused)
        {
            // Bounded by the action table, not the slot count: a slot with no key bound
            // is simply unreachable rather than an index past the end of the table.
            for (int i = 0; i < HotbarActions.Length && i < Hotbar.SlotCount; i++)
                if (Input.IsActionJustPressed(HotbarActions[i]))
                    OnHotbarSlotActivated(i);
        }

        // System spawns (BallSpliter) appear on their own cadence, independent of
        // the serve / paddle — the sim resolves any ball that touches one.
        TickSplitters(delta);

        // Loot rolls on its own separate cadence, and can legitimately produce
        // nothing — see LootTable for how the host's weights decide.
        TickLoot(delta);

        // Paddle: Left/Right arrows. GetAxis returns +1 toward paddle_right.
        // Suspended while the pause menu is open or chat holds the keyboard — the
        // match stays live underneath (a P2P round can't truly pause for everyone)
        // but we stop driving input. In chat's case that is the whole point: the
        // arrows are moving the caret, not the paddle.
        var paddle = _state.Paddle;
        if (!_paused && !_chatFocused)
        {
            float dir = Input.GetAxis("paddle_left", "paddle_right");
            float half = paddle.Width * 0.5f;
            paddle.CenterX = Mathf.Clamp(
                paddle.CenterX + dir * _paddleSpeed * dt, half, _state.ArenaWidth - half);
        }

        // Always advance the simulation so every ball in play keeps moving — with
        // multi-ball, an un-served master resting on the paddle must not freeze the
        // split balls still bouncing around. The held serve ball has zero velocity,
        // so Step leaves it untouched; it's glued to the paddle just below.
        GameSimulation.Step(_state, delta, _step);

        // Hand off any balls that left this screen to the peer across the crossed
        // edge (directed Send, not a broadcast).
        foreach (var handoff in _step.Handoffs)
            _controller?.SendHandoff(handoff);

        foreach (var score in _step.Scores)
            OnScore(score);

        // Loot earned this frame goes to the ball's last hitter — us, or a peer.
        foreach (var pickup in _step.Pickups)
            OnPickupEarned(pickup);

        if (_awaitingServe && _serveBall != null)
        {
            // Rest the un-served ball on the paddle until the player serves it.
            _serveBall.Pos = new Vector2(paddle.CenterX, paddle.Y - _serveBall.Radius);
            if (!_paused && !_chatFocused && Input.IsActionJustPressed("serve"))
            {
                _serveBall.Vel = new Vector2(0, -_serveSpeed);
                // Serving applies force, so it counts as a hit: tag the ball with
                // the server's id (same as a paddle deflection). A later paddle
                // hit by anyone overwrites this, so credit always follows the last
                // player to act on the ball. This lets a clean serve that crosses
                // into a peer's goal untouched score for the server, instead of
                // dying as an "untouched" ball that credited nobody.
                _serveBall.LastHitterId = _state.LocalPlayerId;
                _awaitingServe = false;
                _serveBall = null;
            }
        }

        _view.Render(_state);
        // Scores restate every frame; the board settles its ORDER on its own slower
        // beat, which is the point of the split.
        _leaderboard.SyncFrom(_state);
        // Effect timers restate every frame too — the slot contents they outlive are
        // only repainted on change (SyncFrom), so the two are deliberately separate.
        _hotbar.SyncEffects(_state);
    }
}
