using BriskaBlast.Net;
using Godot;

namespace BriskaBlast.Game;

/// <summary>
/// Rolls the host's loot table to decide what — if anything — a drop tick produces.
///
/// The weighting rule, in one sentence: <b>a weight is a bucket, and items sharing a
/// weight share that bucket.</b> So the odds are computed over the DISTINCT weights of
/// enabled items, and items tied on a weight split their bucket evenly. Whatever is
/// left of 100 is the chance nothing drops at all.
///
/// Worked through:
/// <list type="bullet">
/// <item>Barrier at 50, alone → 50% barrier, 50% nothing.</item>
/// <item>Barrier 50 + another item at 50 → ONE 50 is subscribed, split two ways:
///       25% each, still 50% nothing.</item>
/// <item>10 / 50 / 50 → distinct {10,50} = 60 subscribed → 10% / 25% / 25%, 40% nothing.</item>
/// <item>40 / 40 / 20 → distinct {40,20} = 60 → all three land on 20%, 40% nothing.</item>
/// </list>
///
/// The consequence worth knowing: a weight stops being a literal percentage the moment
/// a second item shares it. The host UI shows resolved rates, not raw weights, so this
/// is visible before a match rather than discovered during one.
///
/// The maths mirrors <c>LootSettings</c> in <c>shared/src/types/loot_settings.rs</c>,
/// which is where it is unit-tested (<c>cargo test -p shared</c>); this side has no
/// test runner, so treat Rust as the reference implementation.
///
/// Every client rolls independently on its own screen, exactly like the BallSpliter
/// spawner — identical odds, different outcomes. Nothing here is networked.
/// </summary>
public static class LootTable
{
    /// <summary>Roll once. Returns null when the roll comes up empty, which is the
    /// normal outcome whenever the host's weights leave headroom under 100.</summary>
    public static ItemId? Roll(LootSettingsDto settings, RandomNumberGenerator rng)
    {
        int total = settings.SubscribedTotal();
        if (total <= 0)
            return null; // every item disabled — nothing can drop

        // One roll across the whole 1..100 space. Landing above the subscribed
        // total is the "nothing drops" outcome, which is what makes a lone item's
        // weight read as its literal percentage.
        int roll = rng.RandiRange(1, LootSettingsDto.WeightTotalMax);
        if (roll > total)
            return null;

        var entries = settings.Entries();
        int accumulated = 0;
        for (int i = 0; i < entries.Length; i++)
        {
            if (!entries[i].Enabled)
                continue;

            // Walk each DISTINCT weight once — a bucket already opened by an
            // earlier tied item is not opened again, which is what stops two items
            // at 50 from subscribing 100 between them.
            if (AlreadyBucketed(entries, i))
                continue;

            accumulated += entries[i].Weight;
            if (roll <= accumulated)
                return PickFromBucket(entries, entries[i].Weight, rng);
        }

        // Unreachable: roll <= total and the buckets sum to exactly total.
        return null;
    }

    // True when an earlier enabled entry already opened this weight's bucket.
    private static bool AlreadyBucketed((bool Enabled, int Weight)[] entries, int index)
    {
        for (int j = 0; j < index; j++)
            if (entries[j].Enabled && entries[j].Weight == entries[index].Weight)
                return true;
        return false;
    }

    // Choose uniformly among the items tied on this weight — the "split it evenly"
    // half of the rule. With one item in the bucket this is just that item.
    private static ItemId PickFromBucket((bool Enabled, int Weight)[] entries, int weight,
        RandomNumberGenerator rng)
    {
        int tied = 0;
        for (int i = 0; i < entries.Length; i++)
            if (entries[i].Enabled && entries[i].Weight == weight)
                tied++;

        int pick = rng.RandiRange(0, tied - 1);
        for (int i = 0; i < entries.Length; i++)
        {
            if (!entries[i].Enabled || entries[i].Weight != weight)
                continue;
            if (pick == 0)
                return ItemRegistry.LootOrder[i];
            pick--;
        }

        // Unreachable: pick < tied, and exactly `tied` entries match.
        return ItemRegistry.LootOrder[0];
    }
}
