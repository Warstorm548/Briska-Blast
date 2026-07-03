using Godot;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using FileAccess = System.IO.FileAccess;

namespace BriskaBlast.Core;

/// <summary>
/// Central, permanent logging for the game client (autoload, registered in
/// <c>project.godot</c> <b>after</b> <see cref="SingleInstance"/> so it can honour
/// its duplicate check). Every line is mirrored to the Godot console (so a
/// terminal run still shows it, and Warn/Error surface in the editor) AND written
/// to a per-run file under the launcher-known per-user data dir, so users can hand
/// us a complete log without launching from a terminal.
///
/// <para><b>Lifecycle — one file per game process:</b> the file opens when this
/// autoload enters the tree (skipped entirely for a rejected duplicate instance,
/// so double-clicks leave no stub) and closes on exit. Menus, match transitions,
/// and disconnects never split it — a whole sitting is one file. Files live in the
/// per-channel folder (<c>logdev</c>/<c>logea</c>/<c>logstable</c>,
/// <see cref="Paths.LogDir"/>), named
/// <c>briskablast_&lt;channel&gt;_&lt;timestamp&gt;.log</c>; the newest
/// <see cref="RetainRuns"/> are kept, older ones pruned on open.</para>
///
/// <para><b>Fail-safe:</b> like <see cref="SingleInstance"/>, nothing here may
/// throw into the game — a logging failure must never wedge play.</para>
/// </summary>
public partial class Log : Node
{
    public enum Level { Trace = 0, Debug = 1, Info = 2, Warn = 3, Error = 4 }

    /// <summary>Run files kept per channel folder; oldest pruned when a new run
    /// opens. Deliberately a small cap so the folder never grows unbounded.</summary>
    private const int RetainRuns = 20;

    /// <summary>Cap on lines buffered before the file opens, so an early failure
    /// (or a duplicate that never opens a file) can't grow memory unbounded.</summary>
    private const int MaxPreOpenLines = 1000;

    private static readonly object Gate = new();
    private static readonly List<string> PreOpenBuffer = new();
    private static StreamWriter? _file;
    private static bool _openAttempted;
    // Log dir resolved during OpenFile, cached so the banner can report it without
    // re-hitting the filesystem (and re-throwing the failure OpenFile already handles).
    private static string? _logDir;

    /// <summary>Minimum level actually recorded. Debug on the dev channel, Info on
    /// ea/stable — so release builds don't spew per-frame detail. Public so a
    /// future in-game toggle can raise/lower verbosity at runtime.</summary>
    public static Level MinLevel { get; set; } =
        BuildConfig.Channel == "dev" ? Level.Debug : Level.Info;

    public static void Trace(string category, string message) => Write(Level.Trace, category, message);
    public static void Debug(string category, string message) => Write(Level.Debug, category, message);
    public static void Info(string category, string message) => Write(Level.Info, category, message);
    public static void Warn(string category, string message) => Write(Level.Warn, category, message);
    public static void Error(string category, string message) => Write(Level.Error, category, message);

    private static void Write(Level level, string category, string message)
    {
        if (level < MinLevel)
            return;

        string line = string.Format(
            CultureInfo.InvariantCulture,
            "[{0:HH:mm:ss.fff}] [{1,-5}] [{2}] {3}",
            DateTime.Now, LevelTag(level), category, message);

        // Mirror to the Godot console so terminal runs still show output and
        // Warn/Error still light up the editor's debugger.
        switch (level)
        {
            case Level.Warn: GD.PushWarning(line); break;
            case Level.Error: GD.PushError(line); break;
            default: GD.Print(line); break;
        }

        lock (Gate)
        {
            if (_file != null)
            {
                try { _file.WriteLine(line); }
                catch { /* fail-safe: never throw into the game */ }
            }
            else if (!_openAttempted)
            {
                PreOpenBuffer.Add(line);
                if (PreOpenBuffer.Count > MaxPreOpenLines)
                    PreOpenBuffer.RemoveAt(0);
            }
            // else: file open failed → keep mirroring to console, drop from disk.
        }
    }

    private static string LevelTag(Level level) => level switch
    {
        Level.Trace => "TRACE",
        Level.Debug => "DEBUG",
        Level.Info => "INFO",
        Level.Warn => "WARN",
        Level.Error => "ERROR",
        _ => "?",
    };

    public override void _EnterTree()
    {
        // A rejected duplicate instance is about to quit — never open a file for
        // it, so the folder only ever holds real sessions.
        if (SingleInstance.IsDuplicate)
            return;

        OpenFile();

        // Log the tail even if the CLR is tearing down under an unhandled
        // exception (AutoFlush already puts prior lines on disk).
        AppDomain.CurrentDomain.UnhandledException += (_, e) =>
        {
            Error("fatal", $"unhandled exception: {e.ExceptionObject}");
            CloseFile();
        };
    }

    public override void _Ready() => EmitBanner();

    public override void _ExitTree() => CloseFile();

    private static void OpenFile()
    {
        lock (Gate)
        {
            if (_openAttempted)
                return;
            _openAttempted = true;
            try
            {
                string dir = Paths.LogDir();
                _logDir = dir;
                PruneOldRuns(dir);
                string name = string.Format(
                    CultureInfo.InvariantCulture,
                    "briskablast_{0}_{1:yyyy-MM-dd_HH-mm-ss}.log",
                    BuildConfig.Channel, DateTime.Now);
                var stream = new FileStream(
                    Path.Combine(dir, name), FileMode.Create, FileAccess.Write, FileShare.Read);
                // AutoFlush so a hang/crash still leaves the tail on disk.
                _file = new StreamWriter(stream, new UTF8Encoding(false)) { AutoFlush = true };
                foreach (string buffered in PreOpenBuffer)
                    _file.WriteLine(buffered);
                PreOpenBuffer.Clear();
            }
            catch (Exception e)
            {
                GD.PushWarning($"Log: failed to open log file: {e.Message}");
                _file = null;
            }
        }
    }

    private static void CloseFile()
    {
        lock (Gate)
        {
            try { _file?.Flush(); _file?.Dispose(); }
            catch { /* fail-safe */ }
            _file = null;
        }
    }

    /// <summary>Keep the newest <see cref="RetainRuns"/> run files: delete oldest
    /// so that after this run opens its file the folder holds at most that many.
    /// Best-effort — retention must never block logging.</summary>
    private static void PruneOldRuns(string dir)
    {
        try
        {
            var files = new DirectoryInfo(dir).GetFiles($"briskablast_{BuildConfig.Channel}_*.log");
            Array.Sort(files, (a, b) => a.LastWriteTimeUtc.CompareTo(b.LastWriteTimeUtc));
            // Delete oldest until at most RetainRuns-1 remain (this run adds one).
            for (int i = 0; i <= files.Length - RetainRuns; i++)
                files[i].Delete();
        }
        catch { /* best-effort */ }
    }

    private static void EmitBanner()
    {
        if (SingleInstance.IsDuplicate)
            return;

        Info("boot",
            $"Briska Blast v{GameVersion.Current} channel={BuildConfig.Channel} " +
            $"platform={OS.GetName()} renderer=\"{RenderingServer.GetVideoAdapterName()}\" " +
            $"resolution={DisplayServer.WindowGetSize()} pid={OS.GetProcessId()}");
        // Reuse the dir OpenFile resolved — never re-call Paths.LogDir() here, so a
        // filesystem failure can't throw out of _Ready and wedge the game.
        Info("boot", $"log_dir={_logDir ?? "(unresolved)"}");
    }
}
