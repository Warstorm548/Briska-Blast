using Godot;
using System.Collections.Generic;
using System.Threading.Tasks;
using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>
/// Process-wide session + identity state (autoload singleton). Holds the
/// channel identity handed over by the launcher, owns the one
/// <see cref="ServerApi"/> instance, and tracks the current lobby roster.
/// </summary>
public partial class SessionContext : Node
{
    public static SessionContext Instance { get; private set; } = null!;

    // ---- Session / lobby state ----
    public string SessionCode { get; set; } = "";
    public string GameMode { get; set; } = "";
    public int MaxPlayers { get; set; }

    /// <summary>Roster as server player_ids, host first. Drives the lobby.</summary>
    public List<string> PlayerIds { get; } = new();
    /// <summary>player_id of the current host (authoritative, from the server).</summary>
    public string HostPlayerId { get; set; } = "";
    public bool LocalPlayerIsHost => HostPlayerId == PlayerId && !string.IsNullOrEmpty(PlayerId);

    // ---- Identity ----
    public string PlayerId { get; private set; } = "";
    public string SecretToken { get; private set; } = "";
    public string LocalUsername { get; private set; } = "Player Username 1";
    public bool HasIdentity => !string.IsNullOrEmpty(PlayerId) && !string.IsNullOrEmpty(SecretToken);

    public ServerApi Api { get; private set; } = null!;

    public override void _Ready()
    {
        Instance = this;

        var handoff = LaunchArgs.FromLauncher;

        if (handoff?.Username?.Trim() is { Length: > 0 } name)
            LocalUsername = name;

        if (handoff?.PlayerId is { Length: > 0 } pid &&
            handoff.SecretToken is { Length: > 0 } token)
        {
            PlayerId = pid;
            SecretToken = token;
            GD.Print($"SessionContext: identity {PlayerId} accepted from launcher.");
        }

        // Defence in depth: the launcher should only ever hand a build the
        // credentials for the channel it was compiled for. A mismatch means
        // something is wrong with the handoff — warn loudly but keep the
        // baked channel as the source of truth.
        if (handoff?.Channel is { Length: > 0 } ch &&
            !ch.Equals(BuildConfig.Channel, System.StringComparison.OrdinalIgnoreCase))
        {
            GD.PushWarning(
                $"SessionContext: handoff channel '{ch}' != build channel '{BuildConfig.Channel}'.");
        }

        // Versions sent on every gated request. Without a launcher handoff
        // (i.e. running from the editor) the launcher version is unknown; in
        // that DEBUG-only dev path we use a sentinel so a fresh local server's
        // version gate doesn't block testing.
        var gameVersion = GameVersion.Current;
        var launcherVersion = handoff?.LauncherVersion ?? "0.0.0";
#if DEBUG
        if (handoff?.PlayerId is null && OS.HasFeature("editor"))
            launcherVersion = "9999.0.0";
#endif

        Api = new ServerApi(gameVersion, launcherVersion);
    }

    /// <summary>
    /// Ensure the client has a usable identity before a server call. With a
    /// launcher handoff this is already satisfied. Running from the editor
    /// (DEBUG, no handoff) it self-registers a throwaway identity so two
    /// editor instances can host/join without the launcher. Release builds
    /// never self-register — they require the launcher's handoff.
    /// </summary>
    public Task<bool> EnsureIdentityAsync()
    {
        if (HasIdentity)
            return Task.FromResult(true);

#if DEBUG
        if (OS.HasFeature("editor"))
            return SelfRegisterAsync();
#endif
        GD.PushWarning("[identity] no launcher identity available — cannot reach the server.");
        return Task.FromResult(false);
    }

#if DEBUG
    // Editor-only: self-provision a throwaway identity so two editor instances
    // can host/join without the launcher. Compiled out of release builds —
    // keeping this off EnsureIdentityAsync's body avoids an async-without-await
    // (CS1998), which the release export treats as a hard error.
    private async Task<bool> SelfRegisterAsync()
    {
        var devName = $"DevTester-{GD.Randi() % 10000}";
        var result = await Api.RegisterAsync(devName);
        if (result.Ok && result.Value is { } reg)
        {
            PlayerId = reg.PlayerId;
            SecretToken = reg.SecretToken;
            LocalUsername = reg.Username;
            GD.Print($"[identity] dev self-registered as {PlayerId} ({LocalUsername}).");
            return true;
        }
        GD.PushWarning($"[identity] dev self-register failed: {result.ErrorCode}");
        return false;
    }
#endif

    /// <summary>Set up local state after a successful POST /host.</summary>
    public void StartHostSession(string code, string mode, int maxPlayers)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        PlayerIds.Clear();
        PlayerIds.Add(PlayerId);
        HostPlayerId = PlayerId;
    }

    /// <summary>Set up local state after a successful POST /join.</summary>
    public void StartJoinSession(string code, string mode, int maxPlayers, IEnumerable<string> roster)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        PlayerIds.Clear();
        PlayerIds.AddRange(roster);
        if (!PlayerIds.Contains(PlayerId))
            PlayerIds.Add(PlayerId);
        // Host identity learned authoritatively from the WS Identified frame.
        HostPlayerId = "";
    }

    public void ClearSession()
    {
        SessionCode = "";
        GameMode = "";
        MaxPlayers = 0;
        PlayerIds.Clear();
        HostPlayerId = "";
    }

    /// <summary>Display name for a roster slot: the local username for self,
    /// otherwise <c>Player &lt;id&gt;</c> (the server roster has no usernames
    /// yet — usernames-in-roster is a documented later enhancement).</summary>
    public string DisplayNameFor(string playerId) =>
        playerId == PlayerId ? LocalUsername : $"Player {playerId}";
}
