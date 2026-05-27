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

public sealed record HostRequest(
    string PlayerId,
    string SecretToken,
    string Gamemode,
    int PlayerCount);

public sealed record HostResponse(string SessionCode);

public sealed record JoinRequest(
    string SessionCode,
    string PlayerId,
    string SecretToken);

public sealed record JoinedPeer(string PlayerId);

public sealed record JoinResponse(
    string Gamemode,
    int PlayerCount,
    int CurrentPlayerCount,
    List<JoinedPeer> Joiners);

public sealed record SessionPollResponse(
    string Status,
    string Gamemode,
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
