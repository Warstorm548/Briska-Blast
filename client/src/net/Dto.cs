using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace BriskaBlast.Net;

/// <summary>
/// C# mirror of the wire types in <c>shared/src/protocol/messages.rs</c>.
/// The Rust <c>shared</c> crate is the single source of truth — these are
/// hand-mirrored because the Godot/C# client can't import the Rust crate.
/// All JSON is snake_case to match serde's <c>rename_all = "snake_case"</c>.
/// </summary>
public static class Json
{
    /// <summary>Shared options: snake_case names, omit null fields on write.</summary>
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };
}

// ---- REST request/response bodies ----

public sealed record RegisterRequest(
    string Username,
    string? PriorPlayerId,
    string? PriorSecretToken);

public sealed record RegisterResponse(
    string PlayerId,
    string SecretToken,
    string Username,
    bool DevFlag);

/// <summary>
/// Mirror of the Rust <c>WinCondition</c> enum (internally tagged on <c>kind</c>).
/// Serializes flat as <c>{"kind":"set_score","target":N}</c> under the snake_case
/// policy. Range constants are hand-mirrored from
/// <c>shared/src/types/win_condition.rs</c> (same single-source convention as the
/// username cap) so the UI input cap and the server's check can't drift.
/// </summary>
public sealed record WinConditionDto(string Kind, int Target)
{
    public const string SetScoreKind = "set_score";
    public const int ScoreMin = 50;
    public const int ScoreMax = 200;
    public const int ScoreDefault = 100;

    public static WinConditionDto SetScore(int target) => new(SetScoreKind, target);
    public static WinConditionDto Default => SetScore(ScoreDefault);
}

/// <summary>
/// Mirror of the Rust <c>SpawnSettings</c> struct. Serializes flat as
/// <c>{"splitter_interval_secs":N,"chain_split":bool}</c> under the snake_case
/// policy. Range constants are hand-mirrored from
/// <c>shared/src/types/spawn_settings.rs</c> (same single-source convention as the
/// win-condition bounds) so the UI slider cap and the server's check can't drift.
/// </summary>
public sealed record SpawnSettingsDto(int SplitterIntervalSecs, bool ChainSplit)
{
    public const int IntervalMin = 5;
    public const int IntervalMax = 60;
    public const int IntervalDefault = 15;
    public const bool ChainSplitDefault = true;

    public static SpawnSettingsDto Default => new(IntervalDefault, ChainSplitDefault);
}

/// <summary>
/// Mirror of the Rust <c>LootSettings</c> struct. Serializes flat as
/// <c>{"drop_interval_secs":N,"barrier_enabled":bool,"barrier_weight":N,
/// "barrier_duration_secs":N}</c> under the snake_case policy. Range constants are
/// hand-mirrored from <c>shared/src/types/loot_settings.rs</c> so the UI slider caps
/// and the server's check can't drift.
///
/// The weighting maths below mirrors the Rust methods of the same names, which are
/// the tested implementation (<c>cargo test -p shared</c>) — this side has no test
/// runner, so treat Rust as the reference and keep the two in step.
/// </summary>
public sealed record LootSettingsDto(
    int DropIntervalSecs,
    bool BarrierEnabled,
    int BarrierWeight,
    int BarrierDurationSecs)
{
    public const int IntervalMin = 5;
    public const int IntervalMax = 60;
    public const int IntervalDefault = 20;

    public const int WeightMin = 1;
    public const int WeightMax = 100;
    /// <summary>The subscribed total can never exceed this — see
    /// <see cref="SubscribedTotal"/> for what "subscribed" means.</summary>
    public const int WeightTotalMax = 100;

    public const int BarrierDurationMin = 5;
    public const int BarrierDurationMax = 120;

    public const bool BarrierEnabledDefault = true;
    public const int BarrierWeightDefault = 50;
    public const int BarrierDurationDefault = 5;

    /// <summary>How many items the loot table holds. Adding item #2 means bumping
    /// this and adding one entry to <see cref="Entries"/>.</summary>
    public const int ItemCount = 1;

    public static LootSettingsDto Default =>
        new(IntervalDefault, BarrierEnabledDefault, BarrierWeightDefault, BarrierDurationDefault);

    /// <summary>Every item's (enabled, weight) pair in a fixed order. The one place
    /// the item list is enumerated; the maths below is already generic over N.</summary>
    public (bool Enabled, int Weight)[] Entries() =>
        new[] { (BarrierEnabled, BarrierWeight) };

    /// <summary>Sum of the <b>distinct</b> weight values among enabled items.
    ///
    /// Distinct, not per-item: two items sharing a weight share one bucket and split
    /// it, so they add that weight to the total once. A lone item at 50 drops on half
    /// of all rolls; two items both at 50 drop on 25% of rolls each — still half the
    /// rolls between them, not all of them. The remainder up to 100 is
    /// <see cref="NothingRate"/>.</summary>
    public int SubscribedTotal()
    {
        var e = Entries();
        int total = 0;
        for (int i = 0; i < e.Length; i++)
        {
            if (!e[i].Enabled)
                continue;
            // Count a weight only the first time it appears among enabled items.
            bool alreadyCounted = false;
            for (int j = 0; j < i; j++)
                if (e[j].Enabled && e[j].Weight == e[i].Weight)
                {
                    alreadyCounted = true;
                    break;
                }
            if (!alreadyCounted)
                total += e[i].Weight;
        }
        return total;
    }

    /// <summary>Each item's actual drop chance as a percentage of all rolls, in
    /// <see cref="Entries"/> order. A disabled item is 0. Items tied on a weight split
    /// it evenly, so these can be fractional even though the weights are integers.</summary>
    public float[] ResolvedRates()
    {
        var e = Entries();
        var rates = new float[e.Length];
        for (int i = 0; i < e.Length; i++)
        {
            if (!e[i].Enabled)
                continue;
            int tied = 0;
            for (int j = 0; j < e.Length; j++)
                if (e[j].Enabled && e[j].Weight == e[i].Weight)
                    tied++;
            rates[i] = e[i].Weight / (float)tied;
        }
        return rates;
    }

    /// <summary>The chance a roll produces nothing at all, as a percentage.</summary>
    public float NothingRate() => System.Math.Max(0, WeightTotalMax - SubscribedTotal());
}

/// <summary>
/// Mirror of the Rust <c>IceServer</c> struct (<c>server/src/turn.rs</c>): one
/// WebRTC <c>iceServers</c> entry, minted by the server from Cloudflare's TURN
/// service and delivered in the <c>start_signaling</c> / <c>identified</c>
/// frames. STUN entries carry no credentials (<c>Username</c>/<c>Credential</c>
/// null); TURN entries carry the short-lived pair. The Cloudflare API token
/// itself never reaches the client.
/// </summary>
public sealed record IceServerDto(string[] Urls, string? Username, string? Credential);

public sealed record HostRequest(
    string PlayerId,
    string SecretToken,
    string Gamemode,
    int PlayerCount,
    WinConditionDto WinCondition,
    SpawnSettingsDto SpawnSettings,
    LootSettingsDto LootSettings);

public sealed record HostResponse(string SessionCode);

public sealed record JoinRequest(
    string SessionCode,
    string PlayerId,
    string SecretToken);

public sealed record JoinedPeer(string PlayerId);

public sealed record JoinResponse(
    string Gamemode,
    WinConditionDto WinCondition,
    SpawnSettingsDto SpawnSettings,
    LootSettingsDto LootSettings,
    int PlayerCount,
    int CurrentPlayerCount,
    List<JoinedPeer> Joiners);

public sealed record SessionPollResponse(
    string Status,
    string Gamemode,
    WinConditionDto WinCondition,
    SpawnSettingsDto SpawnSettings,
    LootSettingsDto LootSettings,
    int PlayerCount,
    int CurrentPlayerCount,
    List<string> JoinerPlayerIds);

public sealed record StartSessionRequest(string PlayerId, string SecretToken);

public sealed record CloseSessionRequest(string PlayerId, string SecretToken);

public sealed record TransferHostRequest(
    string PlayerId,
    string SecretToken,
    string NewHostPlayerId);

// ---- Result wrapper ----

/// <summary>
/// Outcome of an API call. Either a success carrying <typeparamref name="T"/>,
/// or a failure carrying the HTTP status and the server's <c>error</c> code
/// (e.g. <c>"session_full"</c>, <c>"not_found"</c>, <c>"game_update_required"</c>).
/// Network/transport failures use status <c>0</c>.
/// </summary>
public sealed class ApiResult<T>
{
    public bool Ok { get; private init; }
    public T? Value { get; private init; }
    public int Status { get; private init; }
    public string ErrorCode { get; private init; } = "";
    public string ErrorMessage { get; private init; } = "";

    public static ApiResult<T> Success(T value) => new() { Ok = true, Value = value };

    public static ApiResult<T> Failure(int status, string code, string message = "") =>
        new() { Ok = false, Status = status, ErrorCode = code, ErrorMessage = message };
}
