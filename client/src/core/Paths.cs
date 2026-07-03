using Godot;
using System.IO;
using Environment = System.Environment;

namespace BriskaBlast.Core;

/// <summary>
/// Central resolver for the game's per-user writable locations. Kept in one
/// place so the single-instance guard (<see cref="SingleInstance"/>), the logger
/// (<see cref="Log"/>), and anything else that writes all agree on exactly where
/// the launcher probes — the paths here mirror the launcher's Rust
/// <c>directories</c>/<c>paths.rs</c> layout.
/// </summary>
public static class Paths
{
    /// <summary>The per-user data dir: the launcher handoff's value when present,
    /// else the same dir computed locally (editor / standalone run). Mirrors the
    /// Rust <c>ProjectDirs::from("", "", "BriskaBlast").data_dir()</c>.</summary>
    public static string DataDir()
    {
        var handoff = LaunchArgs.FromLauncher;
        if (!string.IsNullOrEmpty(handoff?.DataDir))
            return handoff!.DataDir!;
        return FallbackDataDir();
    }

    /// <summary>Per-channel log folder under <see cref="DataDir"/> — <c>logdev</c>,
    /// <c>logea</c>, <c>logstable</c>. The launcher's Logs button opens the same
    /// path (<c>launcher/src/paths.rs::logs_dir</c>). Created on demand.</summary>
    public static string LogDir()
    {
        var dir = Path.Combine(DataDir(), "log" + BuildConfig.Channel);
        Directory.CreateDirectory(dir);
        return dir;
    }

    /// <summary>Mirrors the Rust <c>directories</c> crate exactly, so a standalone
    /// game writes where a launcher would probe. Note the per-platform asymmetry:
    /// Windows/macOS append <c>/data</c>; Linux lowercases and has no <c>/data</c>
    /// suffix. (A standalone game and a launcher-spawned game running at once is an
    /// unsupported edge — online play is impossible standalone — but both resolve
    /// the same path regardless, so they still agree.)</summary>
    private static string FallbackDataDir()
    {
        if (OS.HasFeature("windows"))
            return Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                "BriskaBlast", "data");
        if (OS.HasFeature("macos"))
            return Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                "Library", "Application Support", "BriskaBlast", "data");
        // Linux: $XDG_DATA_HOME (or ~/.local/share), app name lowercased, no /data.
        string? xdg = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
        string baseDir = !string.IsNullOrEmpty(xdg)
            ? xdg!
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                ".local", "share");
        return Path.Combine(baseDir, "briskablast");
    }
}
