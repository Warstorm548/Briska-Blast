using System;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using System.Threading.Tasks;
using Godot;
// `using Godot;` also imports Godot.HttpClient; alias the System one we want.
using HttpClient = System.Net.Http.HttpClient;

namespace BriskaBlast.Net;

/// <summary>
/// Thin async wrapper over the server's REST endpoints. One instance lives
/// on <c>SessionContext</c> for the process lifetime. Every call attaches
/// the <c>X-Game-Version</c> and <c>X-Launcher-Version</c> headers the
/// version gate checks (<c>server/src/middleware/version.rs</c>).
///
/// Callers get an <see cref="ApiResult{T}"/> instead of exceptions so the
/// UI can branch on the server's <c>error</c> code. NOTE: awaited
/// continuations are not guaranteed to resume on Godot's main thread —
/// callers MUST marshal any scene-tree mutation back via
/// <c>Callable.From(...).CallDeferred()</c>.
/// </summary>
public sealed class ServerApi
{
    private readonly HttpClient _http;

    public ServerApi(string gameVersion, string launcherVersion)
    {
        _http = new HttpClient
        {
            BaseAddress = new Uri(ServerEndpoint.HttpBase + "/"),
            Timeout = TimeSpan.FromSeconds(15),
        };
        _http.DefaultRequestHeaders.Add("X-Game-Version", gameVersion);
        _http.DefaultRequestHeaders.Add("X-Launcher-Version", launcherVersion);
    }

    public Task<ApiResult<RegisterResponse>> RegisterAsync(string username) =>
        SendJson<RegisterRequest, RegisterResponse>(
            HttpMethod.Post, "register", new RegisterRequest(username, null, null));

    public Task<ApiResult<HostResponse>> HostAsync(
        string playerId, string secretToken, string gamemode, int playerCount,
        WinConditionDto winCondition, SpawnSettingsDto spawnSettings,
        LootSettingsDto lootSettings) =>
        SendJson<HostRequest, HostResponse>(
            HttpMethod.Post, "host",
            new HostRequest(playerId, secretToken, gamemode, playerCount, winCondition,
                spawnSettings, lootSettings));

    public Task<ApiResult<JoinResponse>> JoinAsync(
        string code, string playerId, string secretToken) =>
        SendJson<JoinRequest, JoinResponse>(
            HttpMethod.Post, "join",
            new JoinRequest(code, playerId, secretToken));

    public Task<ApiResult<SessionPollResponse>> GetSessionAsync(string code) =>
        SendJson<object, SessionPollResponse>(HttpMethod.Get, $"session/{code}", null);

    public Task<ApiResult<bool>> StartSessionAsync(
        string code, string playerId, string secretToken) =>
        SendNoContent(HttpMethod.Post, $"session/{code}/start",
            new StartSessionRequest(playerId, secretToken));

    public Task<ApiResult<bool>> CloseSessionAsync(
        string code, string playerId, string secretToken) =>
        SendNoContent(HttpMethod.Delete, $"session/{code}",
            new CloseSessionRequest(playerId, secretToken));

    public Task<ApiResult<bool>> TransferHostAsync(
        string code, string playerId, string secretToken, string newHostPlayerId) =>
        SendNoContent(HttpMethod.Post, $"session/{code}/host",
            new TransferHostRequest(playerId, secretToken, newHostPlayerId));

    // ---- internals ----

    private async Task<ApiResult<TResp>> SendJson<TReq, TResp>(
        HttpMethod method, string path, TReq? body)
        where TResp : class
    {
        try
        {
            using var req = new HttpRequestMessage(method, path);
            if (body is not null)
                req.Content = JsonContent.Create(body, options: Json.Options);

            using var resp = await _http.SendAsync(req);
            if (!resp.IsSuccessStatusCode)
                return ApiResult<TResp>.Failure((int)resp.StatusCode, await ReadErrorCode(resp));

            var value = await resp.Content.ReadFromJsonAsync<TResp>(Json.Options);
            return value is null
                ? ApiResult<TResp>.Failure(0, "empty_body")
                : ApiResult<TResp>.Success(value);
        }
        catch (Exception e)
        {
            GD.PushWarning($"ServerApi {method} {path} failed: {e.Message}");
            return ApiResult<TResp>.Failure(0, "network_error", e.Message);
        }
    }

    private async Task<ApiResult<bool>> SendNoContent<TReq>(
        HttpMethod method, string path, TReq body)
    {
        try
        {
            using var req = new HttpRequestMessage(method, path)
            {
                Content = JsonContent.Create(body, options: Json.Options),
            };

            using var resp = await _http.SendAsync(req);
            return resp.IsSuccessStatusCode
                ? ApiResult<bool>.Success(true)
                : ApiResult<bool>.Failure((int)resp.StatusCode, await ReadErrorCode(resp));
        }
        catch (Exception e)
        {
            GD.PushWarning($"ServerApi {method} {path} failed: {e.Message}");
            return ApiResult<bool>.Failure(0, "network_error", e.Message);
        }
    }

    /// Pull the server's <c>{"error": "..."}</c> discriminator out of a
    /// failure body. Falls back to the bare status when the body isn't the
    /// expected shape.
    private static async Task<string> ReadErrorCode(HttpResponseMessage resp)
    {
        try
        {
            var text = await resp.Content.ReadAsStringAsync();
            using var doc = JsonDocument.Parse(text);
            if (doc.RootElement.TryGetProperty("error", out var err))
                return err.GetString() ?? "unknown";
        }
        catch
        {
            // non-JSON / empty body — fall through
        }
        return $"http_{(int)resp.StatusCode}";
    }
}
