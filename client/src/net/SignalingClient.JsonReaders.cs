using System;
using System.Collections.Generic;
using System.Text.Json;

namespace BriskaBlast.Net;

/// <summary>Deliberately tolerant JSON readers. Every one degrades to a default on
/// an absent or malformed field, which is how a client keeps working against a
/// server that predates the field rather than throwing on the frame.</summary>
public partial class SignalingClient
{
    private static string Str(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) ? el.GetString() ?? "" : "";

    private static int IntProp(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.TryGetInt32(out var v) ? v : 0;

    private static long LongProp(JsonElement obj, string name) =>
        obj.TryGetProperty(name, out var el) && el.TryGetInt64(out var v) ? v : 0;

    private static Dictionary<string, int> ReadIntMap(JsonElement obj, string name)
    {
        var map = new Dictionary<string, int>();
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
            foreach (var prop in el.EnumerateObject())
                if (prop.Value.TryGetInt32(out var v))
                    map[prop.Name] = v;
        return map;
    }

    /// <summary>Read a JSON object of string→string into a dictionary. Returns an
    /// empty map when the property is absent (so a client talking to a server
    /// that predates the field degrades gracefully rather than throwing).</summary>
    private static Dictionary<string, string> ReadStringMap(JsonElement obj, string name)
    {
        var map = new Dictionary<string, string>();
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
            foreach (var prop in el.EnumerateObject())
                if (prop.Value.ValueKind == JsonValueKind.String)
                    map[prop.Name] = prop.Value.GetString() ?? "";
        return map;
    }

    /// <summary>Read a <c>win_condition</c> object (<c>{kind,target}</c>) into a
    /// DTO. A missing/malformed field degrades to the default so a client talking
    /// to a server that predates the field still has a usable rule.</summary>
    private static WinConditionDto ReadWinCondition(JsonElement obj, string name)
    {
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
        {
            string kind = el.TryGetProperty("kind", out var k) && k.ValueKind == JsonValueKind.String
                ? k.GetString() ?? WinConditionDto.SetScoreKind
                : WinConditionDto.SetScoreKind;
            int target = el.TryGetProperty("target", out var t) && t.TryGetInt32(out var v)
                ? v
                : WinConditionDto.ScoreDefault;
            return new WinConditionDto(kind, target);
        }
        return WinConditionDto.Default;
    }

    /// <summary>Read a <c>spawn_settings</c> object into a DTO. A missing/malformed
    /// field degrades to the default so a client talking to a server that predates
    /// the field still has usable random-spawn rules.</summary>
    private static SpawnSettingsDto ReadSpawnSettings(JsonElement obj, string name)
    {
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
        {
            int interval = el.TryGetProperty("splitter_interval_secs", out var s) && s.TryGetInt32(out var v)
                ? v
                : SpawnSettingsDto.IntervalDefault;
            bool chain = el.TryGetProperty("chain_split", out var c)
                && (c.ValueKind == JsonValueKind.True || c.ValueKind == JsonValueKind.False)
                ? c.ValueKind == JsonValueKind.True
                : SpawnSettingsDto.ChainSplitDefault;
            return new SpawnSettingsDto(interval, chain);
        }
        return SpawnSettingsDto.Default;
    }

    /// <summary>Read a <c>loot_settings</c> object into a DTO. A missing/malformed
    /// field degrades to the default so a client talking to a server that predates
    /// the field still has a usable loot table. Note this only covers a stale
    /// SERVER — a stale CLIENT never reads the field at all, which is why the
    /// feature is gated on <c>min_game_version</c> rather than left to degrade.</summary>
    private static LootSettingsDto ReadLootSettings(JsonElement obj, string name)
    {
        if (obj.TryGetProperty(name, out var el) && el.ValueKind == JsonValueKind.Object)
        {
            int interval = el.TryGetProperty("drop_interval_secs", out var d) && d.TryGetInt32(out var iv)
                ? iv
                : LootSettingsDto.IntervalDefault;
            bool enabled = el.TryGetProperty("barrier_enabled", out var e)
                && (e.ValueKind == JsonValueKind.True || e.ValueKind == JsonValueKind.False)
                ? e.ValueKind == JsonValueKind.True
                : LootSettingsDto.BarrierEnabledDefault;
            int weight = el.TryGetProperty("barrier_weight", out var w) && w.TryGetInt32(out var wv)
                ? wv
                : LootSettingsDto.BarrierWeightDefault;
            int duration = el.TryGetProperty("barrier_duration_secs", out var s) && s.TryGetInt32(out var sv)
                ? sv
                : LootSettingsDto.BarrierDurationDefault;
            return new LootSettingsDto(interval, enabled, weight, duration);
        }
        return LootSettingsDto.Default;
    }

    /// <summary>Read the <c>ice_servers</c> array (server-minted STUN+TURN
    /// entries) into DTOs. Absent/malformed — an old server, TURN unconfigured,
    /// or a failed mint — degrades to an empty array; the transport then keeps
    /// its built-in STUN-only fallback. Entries without a usable <c>urls</c>
    /// array are skipped.</summary>
    private static IceServerDto[] ReadIceServers(JsonElement obj)
    {
        if (!obj.TryGetProperty("ice_servers", out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<IceServerDto>();
        var list = new List<IceServerDto>(arr.GetArrayLength());
        foreach (var item in arr.EnumerateArray())
        {
            if (item.ValueKind != JsonValueKind.Object)
                continue;
            var urls = ReadStrings(item, "urls");
            if (urls.Length == 0)
                continue;
            string? username = item.TryGetProperty("username", out var u) && u.ValueKind == JsonValueKind.String
                ? u.GetString()
                : null;
            string? credential = item.TryGetProperty("credential", out var c) && c.ValueKind == JsonValueKind.String
                ? c.GetString()
                : null;
            list.Add(new IceServerDto(urls, username, credential));
        }
        return list.ToArray();
    }

    private static string[] ReadStrings(JsonElement obj, string name)
    {
        if (!obj.TryGetProperty(name, out var arr) || arr.ValueKind != JsonValueKind.Array)
            return Array.Empty<string>();
        var list = new List<string>(arr.GetArrayLength());
        foreach (var item in arr.EnumerateArray())
            list.Add(item.GetString() ?? "");
        return list.ToArray();
    }
}
