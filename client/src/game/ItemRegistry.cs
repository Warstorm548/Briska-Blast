using BriskaBlast.Core;

namespace BriskaBlast.Game;

/// <summary>Every actionable item that can occupy a hotbar slot. Numbers count
/// upward and are never reused or reordered — the same rule as <see cref="AssetId"/>,
/// for the same reason (an id may end up persisted or on the wire).</summary>
public enum ItemId
{
    /// <summary>Deploys a full-width barrier below the paddle that blocks every ball
    /// which would otherwise score, for a host-configured stacking duration.</summary>
    BarrierShield = 1,
}

/// <summary>One item's fixed properties.</summary>
public readonly struct ItemEntry
{
    public readonly ItemId Id;
    /// <summary>Shown in the hotbar's active-effect readout. Player-facing.</summary>
    public readonly string DisplayName;
    /// <summary>Art for both the hotbar slot icon and the collectible on the field —
    /// deliberately one sprite, so what a player sees lying in the arena is exactly
    /// what they then see in their bar.</summary>
    public readonly AssetId Icon;
    /// <summary>How many of this item one slot may hold.</summary>
    public readonly int MaxStack;

    public ItemEntry(ItemId id, string displayName, AssetId icon, int maxStack)
    {
        Id = id;
        DisplayName = displayName;
        Icon = icon;
        MaxStack = maxStack;
    }
}

/// <summary>
/// The item lookup table — the "future item lookup table" <see cref="ItemSlot"/>'s
/// doc comment defers max-stack to. Deliberately shaped like
/// <see cref="SpriteRegistry"/>: a static row table plus an id-keyed lookup, so an
/// item's rules live in exactly one place and a slot never decides its own cap.
///
/// Pure data, so this is a plain static class rather than an autoload Node —
/// <see cref="SpriteRegistry"/> only needs to be a Node because it caches loaded
/// textures.
/// </summary>
public static class ItemRegistry
{
    private static readonly ItemEntry[] Entries =
    {
        new(ItemId.BarrierShield, "Full Barrier", AssetId.BarrierShield, 5),
    };

    /// <summary>The loot table's items, in the SAME order as
    /// <see cref="BriskaBlast.Net.LootSettingsDto.Entries"/>. The two must agree — a weight at
    /// index i belongs to the item at index i.
    ///
    /// Sized by <c>LootSettingsDto.ItemCount</c> on purpose: adding an item to one
    /// side and not the other is then a compile error, not a silently mis-assigned
    /// drop weight.</summary>
    public static readonly ItemId[] LootOrder = new ItemId[BriskaBlast.Net.LootSettingsDto.ItemCount]
    {
        ItemId.BarrierShield,
    };

    /// <summary>Look up an item's row. Every <see cref="ItemId"/> has one.</summary>
    public static ItemEntry Get(ItemId id)
    {
        foreach (var e in Entries)
            if (e.Id == id)
                return e;
        // Unreachable for any declared ItemId; returning the first row keeps a
        // corrupt id from taking the match down mid-rally.
        return Entries[0];
    }

    public static string DisplayName(ItemId id) => Get(id).DisplayName;
    public static AssetId Icon(ItemId id) => Get(id).Icon;
    public static int MaxStack(ItemId id) => Get(id).MaxStack;

    /// <summary>Map an <see cref="AssetId"/> icon back to its item. The hotbar stores
    /// an <c>AssetId</c> per slot (see <see cref="ItemSlot.Icon"/>), so this is what
    /// turns a slot back into the item it holds when it is activated.</summary>
    public static bool TryFromIcon(AssetId icon, out ItemId id)
    {
        foreach (var e in Entries)
            if (e.Icon == icon)
            {
                id = e.Id;
                return true;
            }
        id = default;
        return false;
    }
}
