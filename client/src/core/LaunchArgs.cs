using Godot;
using System;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace BriskaBlast.Core;

public static class LaunchArgs
{
    private const string HandoffFlag = "--launcher-handoff";

    /// <summary>
    /// One-shot identity + version handoff written by the launcher (see
    /// <c>launcher/src/game_launch/mod.rs</c>). The snake_case wire keys are
    /// mapped explicitly so this stays decoupled from the net layer's JSON
    /// options. <see cref="PlayerId"/>/<see cref="SecretToken"/> authenticate
    /// the client to the server; <see cref="LauncherVersion"/> feeds the
    /// version gate; <see cref="Channel"/> lets the client assert it wasn't
    /// handed cross-channel credentials.
    /// </summary>
    public sealed record Handoff(
        string? Username,
        [property: JsonPropertyName("player_id")] string? PlayerId,
        [property: JsonPropertyName("secret_token")] string? SecretToken,
        [property: JsonPropertyName("launcher_version")] string? LauncherVersion,
        string? Channel);

    private static Handoff? _cached;
    private static bool _loaded;

    public static Handoff? FromLauncher
    {
        get
        {
            if (!_loaded)
            {
                _cached = LoadOnce();
                _loaded = true;
            }
            return _cached;
        }
    }

    private static Handoff? LoadOnce()
    {
        var path = FindHandoffPath(OS.GetCmdlineArgs());
        if (path is null) return null;

        Handoff? consumed;
        try
        {
            var json = File.ReadAllText(path);
            var parsed = JsonSerializer.Deserialize<Handoff>(
                json,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            consumed = parsed ?? new Handoff(null, null, null, null, null);
        }
        catch (Exception e)
        {
            GD.PushWarning($"LaunchArgs: failed to read handoff file '{path}': {e.Message}");
            // Do NOT delete on failure — the path came from --launcher-handoff
            // which is user-supplied at the process boundary. If the read
            // failed (wrong path, malformed file, etc) the file may belong
            // to something else; the launcher's normal-case handoff file
            // is uuid-named under tmp and will be garbage-collected by the
            // OS anyway. The launcher itself also cleans the file on exit
            // (game_launch::spawn_and_wait) as a backstop.
            return null;
        }
        // Only reached on successful read+parse — safe to delete because
        // the file's shape matches the handoff schema we wrote.
        TryDelete(path);
        return consumed;
    }

    private static string? FindHandoffPath(string[] args)
    {
        for (var i = 0; i < args.Length; i++)
        {
            if (args[i] == HandoffFlag && i + 1 < args.Length)
                return args[i + 1];

            if (args[i].StartsWith(HandoffFlag + "="))
                return args[i].Substring(HandoffFlag.Length + 1);
        }
        return null;
    }

    private static void TryDelete(string path)
    {
        try { File.Delete(path); }
        catch (Exception e)
        {
            GD.PushWarning($"LaunchArgs: failed to delete handoff file '{path}': {e.Message}");
        }
    }
}
