using Godot;
using System;
using System.IO;
using System.Text.Json;

namespace BriskaBlast.Core;

public static class LaunchArgs
{
    private const string HandoffFlag = "--launcher-handoff";

    public sealed record Handoff(string? Username);

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

        try
        {
            var json = File.ReadAllText(path);
            var parsed = JsonSerializer.Deserialize<Handoff>(
                json,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            return parsed ?? new Handoff(null);
        }
        catch (Exception e)
        {
            GD.PushWarning($"LaunchArgs: failed to read handoff file '{path}': {e.Message}");
            return null;
        }
        finally
        {
            TryDelete(path);
        }
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
