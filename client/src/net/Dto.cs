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
    public const int ScoreMin = 10;
    public const int ScoreMax = 50;
    public const int ScoreDefault = 11;

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
    SpawnSettingsDto SpawnSettings);

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
    int PlayerCount,
    int CurrentPlayerCount,
    List<JoinedPeer> Joiners);

public sealed record SessionPollResponse(
    string Status,
    string Gamemode,
    WinConditionDto WinCondition,
    SpawnSettingsDto SpawnSettings,
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
