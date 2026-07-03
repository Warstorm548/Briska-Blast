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

    /// <summary>The match-end rule for this session (kind + target score), learned
    /// from host setup, the join/poll response, or the <c>start_signaling</c> frame.
    /// Mirrors the server's <c>win_condition</c>. <see cref="WinConditionDisplay"/>
    /// formats it for UI (e.g. the end-game screen).</summary>
    public string WinConditionKind { get; set; } = WinConditionDto.SetScoreKind;
    public int WinScoreTarget { get; set; } = WinConditionDto.ScoreDefault;

    /// <summary>Random-spawn rules for this session (BallSpliter cadence +
    /// chain-split), learned from host setup, the join/poll response, or the
    /// <c>start_signaling</c> frame. Mirror the server's <c>spawn_settings</c>;
    /// <see cref="Game.GameScene"/> drives its local spawner from these.</summary>
    public int SplitterIntervalSecs { get; set; } = SpawnSettingsDto.IntervalDefault;
    public bool ChainSplit { get; set; } = SpawnSettingsDto.ChainSplitDefault;

    /// <summary>Human-readable win condition for menus, e.g. "First to 11".</summary>
    public string WinConditionDisplay =>
        WinConditionKind == WinConditionDto.SetScoreKind
            ? $"First to {WinScoreTarget}"
            : WinConditionKind;

    /// <summary>Roster as server player_ids, host first. Drives the lobby.</summary>
    public List<string> PlayerIds { get; } = new();
    /// <summary>Frozen Extended-mode seating roster — `[host, …joiners]` in join
    /// order, self-inclusive — captured once at match Start (from
    /// <c>start_signaling</c>) or on a process-death rejoin (from the
    /// <c>Identified</c> frame's <c>seat_order</c>). Drives
    /// <see cref="Game.GameScene"/> portal layout; empty until a match starts and
    /// identical on every client, so the layout matches across screens and is
    /// unaffected by a later host promotion. Cleared when a session starts or ends.</summary>
    public List<string> SeatOrder { get; } = new();
    /// <summary>player_id of the current host (authoritative, from the server).</summary>
    public string HostPlayerId { get; set; } = "";
    public bool LocalPlayerIsHost => HostPlayerId == PlayerId && !string.IsNullOrEmpty(PlayerId);

    /// <summary>player_id → server-provided display name, learned from the
    /// signaling Identified / PeerJoined frames. Backs <see cref="DisplayNameFor"/>
    /// so the lobby roster and in-game scoreboard show usernames rather than the
    /// internal id. Additive within a session (not pruned on PeerLeft, so a
    /// departed player who still holds points keeps their name on the scoreboard);
    /// cleared whenever a session starts or ends.</summary>
    private readonly Dictionary<string, string> _usernames = new();

    // ---- Identity ----
    public string PlayerId { get; private set; } = "";
    public string SecretToken { get; private set; } = "";
    public string LocalUsername { get; private set; } = "Player Username 1";
    public bool HasIdentity => !string.IsNullOrEmpty(PlayerId) && !string.IsNullOrEmpty(SecretToken);

    public ServerApi Api { get; private set; } = null!;

    /// <summary>True while we're rejoining a match already in progress (entered
    /// from the Join screen, not the lobby Start). <see cref="Game.GameScene"/>
    /// reads this in <c>_Ready</c> to skip the host's first serve — the ball is
    /// already in play elsewhere — then clears it.</summary>
    public bool RejoinInProgress { get; set; }

    // ---- Live networking, carried across the lobby → game scene change ----
    // The lobby creates the signaling socket + WebRTC mesh; on start_signaling
    // it hands them here via AdoptNet so they survive ChangeSceneToFile and the
    // game scene keeps using the same connection. Null until adopted.
    public SignalingClient? Signaling { get; private set; }
    public IPeerTransport? Transport { get; private set; }

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
            Log.Info("session", $"identity {PlayerId} accepted from launcher.");
        }

        // Defence in depth: the launcher should only ever hand a build the
        // credentials for the channel it was compiled for. A mismatch means
        // something is wrong with the handoff — warn loudly but keep the
        // baked channel as the source of truth.
        if (handoff?.Channel is { Length: > 0 } ch &&
            !ch.Equals(BuildConfig.Channel, System.StringComparison.OrdinalIgnoreCase))
        {
            Log.Warn("session",
                $"handoff channel '{ch}' != build channel '{BuildConfig.Channel}'.");
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
        Log.Warn("identity", "no launcher identity available — cannot reach the server.");
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
            Log.Info("identity", $"dev self-registered as {PlayerId} ({LocalUsername}).");
            return true;
        }
        Log.Warn("identity", $"dev self-register failed: {result.ErrorCode}");
        return false;
    }
#endif

    /// <summary>Set up local state after a successful POST /host.</summary>
    public void StartHostSession(string code, string mode, int maxPlayers,
        WinConditionDto winCondition, SpawnSettingsDto spawnSettings)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        ApplyWinCondition(winCondition);
        ApplySpawnSettings(spawnSettings);
        PlayerIds.Clear();
        SeatOrder.Clear();
        _usernames.Clear();
        PlayerIds.Add(PlayerId);
        HostPlayerId = PlayerId;
    }

    /// <summary>Set up local state after a successful POST /join.</summary>
    public void StartJoinSession(string code, string mode, int maxPlayers,
        WinConditionDto winCondition, SpawnSettingsDto spawnSettings, IEnumerable<string> roster)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        ApplyWinCondition(winCondition);
        ApplySpawnSettings(spawnSettings);
        PlayerIds.Clear();
        SeatOrder.Clear();
        _usernames.Clear();
        PlayerIds.AddRange(roster);
        if (!PlayerIds.Contains(PlayerId))
            PlayerIds.Add(PlayerId);
        // Host identity learned authoritatively from the WS Identified frame.
        HostPlayerId = "";
    }

    /// <summary>Set up local state for rejoining a live match by code (process-
    /// death recovery). The roster and host arrive authoritatively in the WS
    /// <c>Identified</c> frame, so they start empty here.</summary>
    public void StartRejoinSession(string code, string mode, int maxPlayers,
        WinConditionDto winCondition, SpawnSettingsDto spawnSettings)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        ApplyWinCondition(winCondition);
        ApplySpawnSettings(spawnSettings);
        PlayerIds.Clear();
        SeatOrder.Clear();
        _usernames.Clear();
        HostPlayerId = "";
        RejoinInProgress = true;
    }

    /// <summary>Capture the server-authoritative, frozen seating roster (join
    /// order, self-inclusive) for Extended-mode portal layout. Set once at match
    /// Start or on rejoin; see <see cref="SeatOrder"/>.</summary>
    public void SetSeatOrder(IEnumerable<string> order)
    {
        SeatOrder.Clear();
        SeatOrder.AddRange(order);
    }

    /// <summary>Adopt a win condition from a wire DTO (host/join/poll). A null DTO
    /// (a server predating the field) falls back to the default so the game still
    /// has a usable rule.</summary>
    public void ApplyWinCondition(WinConditionDto? winCondition)
    {
        var wc = winCondition ?? WinConditionDto.Default;
        ApplyWinCondition(wc.Kind, wc.Target);
    }

    /// <summary>Adopt a win condition by its parsed fields (used by the
    /// <c>start_signaling</c> frame). An empty kind falls back to the default, and a
    /// target outside the shared bounds (a server predating the field, or an
    /// out-of-range value) falls back to the default rather than displaying a
    /// nonsense score.</summary>
    public void ApplyWinCondition(string kind, int target)
    {
        WinConditionKind = string.IsNullOrEmpty(kind) ? WinConditionDto.SetScoreKind : kind;
        WinScoreTarget = target >= WinConditionDto.ScoreMin && target <= WinConditionDto.ScoreMax
            ? target
            : WinConditionDto.ScoreDefault;
    }

    /// <summary>Adopt random-spawn settings from a wire DTO (host/join/poll). A null
    /// DTO (a server predating the field) falls back to the defaults.</summary>
    public void ApplySpawnSettings(SpawnSettingsDto? spawnSettings)
    {
        var s = spawnSettings ?? SpawnSettingsDto.Default;
        ApplySpawnSettings(s.SplitterIntervalSecs, s.ChainSplit);
    }

    /// <summary>Adopt random-spawn settings by parsed fields (used by the
    /// <c>start_signaling</c> frame). An out-of-range interval falls back to the
    /// default rather than driving a nonsense spawner cadence.</summary>
    public void ApplySpawnSettings(int splitterIntervalSecs, bool chainSplit)
    {
        SplitterIntervalSecs =
            splitterIntervalSecs >= SpawnSettingsDto.IntervalMin && splitterIntervalSecs <= SpawnSettingsDto.IntervalMax
                ? splitterIntervalSecs
                : SpawnSettingsDto.IntervalDefault;
        ChainSplit = chainSplit;
    }

    /// <summary>Reset all per-session state (code, mode, roster, seating, usernames,
    /// host, rejoin flag) back to empty — called when a match ends or is left.</summary>
    public void ClearSession()
    {
        SessionCode = "";
        GameMode = "";
        MaxPlayers = 0;
        WinConditionKind = WinConditionDto.SetScoreKind;
        WinScoreTarget = WinConditionDto.ScoreDefault;
        SplitterIntervalSecs = SpawnSettingsDto.IntervalDefault;
        ChainSplit = SpawnSettingsDto.ChainSplitDefault;
        PlayerIds.Clear();
        SeatOrder.Clear();
        _usernames.Clear();
        HostPlayerId = "";
        RejoinInProgress = false;
    }

    /// <summary>Reparent the live signaling + transport under this autoload so
    /// they outlive the lobby scene. Called by the lobby on start_signaling,
    /// synchronously before it changes to the game scene.</summary>
    public void AdoptNet(SignalingClient signaling, IPeerTransport? transport)
    {
        Reparent(signaling);
        if (transport is Node transportNode)
            Reparent(transportNode);
        Signaling = signaling;
        Transport = transport;
    }

    private void Reparent(Node node)
    {
        node.GetParent()?.RemoveChild(node);
        AddChild(node);
    }

    /// <summary>Close and free the adopted signaling + transport (game exit or
    /// session end). No-op when nothing was adopted.</summary>
    public void TeardownNet()
    {
        Transport?.Close();
        if (Transport is Node transportNode)
            transportNode.QueueFree();
        Transport = null;

        Signaling?.CloseConnection();
        Signaling?.QueueFree();
        Signaling = null;
    }

    /// <summary>Record one peer's server-provided display name (from a
    /// PeerJoined frame). Empty ids/names are ignored so a player with no
    /// username on file falls through to the <c>Player &lt;id&gt;</c> fallback.</summary>
    public void SetUsername(string playerId, string username)
    {
        if (!string.IsNullOrEmpty(playerId) && !string.IsNullOrEmpty(username))
            _usernames[playerId] = username;
    }

    /// <summary>Merge a player_id → username map (from an Identified roster
    /// snapshot) into the known names. Empty names are skipped.</summary>
    public void MergeUsernames(IReadOnlyDictionary<string, string> usernames)
    {
        foreach (var (id, name) in usernames)
            SetUsername(id, name);
    }

    /// <summary>Display name for a roster slot: the local username for self, the
    /// server-provided username once we've learned one for this id, otherwise the
    /// <c>Player &lt;id&gt;</c> fallback. The numeric id is an internal key and is
    /// only surfaced through this fallback when no username is available.</summary>
    public string DisplayNameFor(string playerId)
    {
        if (playerId == PlayerId)
            return LocalUsername;
        if (_usernames.TryGetValue(playerId, out var name) && !string.IsNullOrEmpty(name))
            return name;
        return $"Player {playerId}";
    }
}
