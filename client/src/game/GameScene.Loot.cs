using Godot;
using BriskaBlast.Core;
using BriskaBlast.Net;

namespace BriskaBlast.Game;

/// <summary>The loot-drop cadence and everything an item does after it drops:
/// crediting the earner, banking it in the hotbar, and spending it.</summary>
public partial class GameScene
{
    // Loot-drop cadence and the host's table. Like the splitter spawner above, each
    // screen rolls its own drops locally — identical odds everywhere, independent
    // outcomes. Seeded from the host's settings in _Ready.
    private LootSettingsDto _lootSettings = LootSettingsDto.Default;
    private double _lootIntervalSecs = LootSettingsDto.IntervalDefault;
    private double _lootCooldown;

    /// <summary>Count down the loot timer and roll the host's table when it expires.
    /// Modelled on <see cref="TickSplitters"/> — its own cadence, the same placement
    /// rules — with two differences: the roll can legitimately come up empty, and at
    /// most one uncollected pickup is allowed on screen at a time.
    ///
    /// The one-at-a-time cap keeps items from piling up when nobody is collecting
    /// them (a stack that is already full, or a rally that never reaches the drop).</summary>
    private void TickLoot(double dt)
    {
        if (_lootIntervalSecs <= 0)
            return; // disabled by the host

        _lootCooldown -= dt;
        if (_lootCooldown > 0)
            return;
        _lootCooldown = _lootIntervalSecs;

        if (_state.Pickups.Count > 0)
            return; // one uncollected pickup at a time

        var rolled = LootTable.Roll(_lootSettings, _rng);
        if (rolled is not { } item)
            return; // the roll came up empty — the normal case under 100% subscribed

        // Same placement rules as a splitter: clear of the edges and the paddle band,
        // and out of the corner barriers so it can't spawn unreachable.
        float margin = _ballRadius * 4f;
        float minX = margin, maxX = _state.ArenaWidth - margin;
        float minY = margin, maxY = _state.Paddle.Y - margin;
        if (maxX <= minX || maxY <= minY)
            return;

        float radius = _ballRadius * 1.5f;
        for (int attempt = 0; attempt < 8; attempt++)
        {
            var pos = new Vector2(_rng.RandfRange(minX, maxX), _rng.RandfRange(minY, maxY));
            if (OverlapsBarrier(pos, radius))
                continue;
            _state.Pickups.Add(new Pickup
            {
                Id = _state.NextPickupId(),
                Radius = radius,
                Pos = pos,
                Item = item,
            });
            Log.Debug("game.loot", $"spawned {item} at ({pos.X:F0},{pos.Y:F0})");
            return;
        }
    }

    /// <summary>A ball touched a pickup on our screen. The item belongs to that ball's
    /// last hitter, so it either lands in our own hotbar or is sent to the peer who
    /// earned it. The sim has already consumed the pickup by this point.</summary>
    private void OnPickupEarned(PickupEvent ev)
    {
        if (ev.EarnerId == _state.LocalPlayerId)
        {
            GrantItem(ev.Item);
            return;
        }

        _controller?.SendItemAward(ev.EarnerId, ev.Item);
    }

    /// <summary>Put an earned item in the local hotbar and refresh the bar. Silently
    /// does nothing when the bar has no room — our own stack cap, applied identically
    /// whether the item was earned here or awarded by a peer.</summary>
    private void GrantItem(ItemId item)
    {
        if (!_state.Hotbar.TryAdd(item))
        {
            Log.Debug("game.loot", $"earned {item} but the hotbar is full — item lost");
            return;
        }
        _hotbar.SyncFrom(_state.Hotbar);
    }

    /// <summary>A hotbar slot's key was pressed. Acknowledges the press on screen, then
    /// spends one charge from the slot and applies the item's effect.</summary>
    private void OnHotbarSlotActivated(int index)
    {
        // Flash regardless of contents: the player pressed a key and deserves to see
        // that it registered, whether or not the slot had anything in it.
        _hotbar.Flash(index);

        var slot = _state.Hotbar.Slots[index];
        if (slot.IsEmpty)
        {
            Log.Debug("game.hotbar", $"slot {index + 1} pressed (empty)");
            return;
        }

        // Consume returns what was spent, because spending the last charge clears
        // the slot — reading the icon afterwards would find nothing.
        if (_state.Hotbar.Consume(index) is not { } item)
            return;

        switch (item)
        {
            case ItemId.BarrierShield:
                // ADD to whatever is left rather than replacing it: activating at 15s
                // remaining leaves 45s, which is what makes holding a stack worth
                // something mid-rally.
                _state.ShieldSecsRemaining += _lootSettings.BarrierDurationSecs;
                break;
        }

        _hotbar.SyncFrom(_state.Hotbar);
        Log.Debug("game.hotbar",
            $"slot {index + 1} activated {item} (shield now {_state.ShieldSecsRemaining:F1}s)");
    }
}
