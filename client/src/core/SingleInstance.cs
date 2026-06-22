using Godot;
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
// Disambiguate from Godot's same-named types pulled in by `using Godot;`.
using FileAccess = System.IO.FileAccess;
using Environment = System.Environment;

namespace BriskaBlast.Core;

/// <summary>
/// Game single-instance + one-channel-at-a-time guard (autoload, registered in
/// <c>project.godot</c> <b>above</b> <see cref="SessionContext"/> so it runs
/// first). Mirror of the launcher's socket rendezvous
/// (<c>launcher/src/rendezvous.rs</c>): bind <c>127.0.0.1:0</c> (a guaranteed-free
/// ephemeral port — no fixed port to collide with strangers' software), record
/// the port in a single shared <c>game_instance.json</c> via an atomic
/// create-new, and serve a handshake banner so a second game of <i>any</i>
/// channel detects the live one and quits. Defence-in-depth alongside the
/// launcher's <c>game_running</c> gate — both are intended.
///
/// <para><b>Fail-safe is the prime directive:</b> any inconclusive error here
/// logs and lets the game run normally. Nothing here may wedge the game shut.</para>
/// </summary>
public partial class SingleInstance : Node
{
    /// <summary>Wire protocol version. MUST match <c>RENDEZVOUS_PROTO</c> in
    /// <c>launcher/src/rendezvous.rs</c>.</summary>
    private const int Proto = 1;
    private const string Role = "GAME";
    private const string FileName = "game_instance.json";
    private const int ProbeTimeoutMs = 200;
    private const int AcquireAttempts = 5;
    // A freshly create-new'd file is momentarily empty before its owner writes
    // the port; re-read briefly before declaring it stale so that window can't
    // trigger a false reclaim (mirrors the Rust read_port_with_retry).
    private const int PortReadAttempts = 10;
    private const int PortReadBackoffMs = 10;

    /// <summary>True when this process found a live game already running and is
    /// quitting. The main scene checks this at the very top of its
    /// <c>_Ready</c> and bails immediately — belt-and-braces in case the deferred
    /// quit lets a frame slip.</summary>
    public static bool IsDuplicate { get; private set; }

    private TcpListener? _listener;
    private bool _ownsFile;
    private string? _filePath;
    private volatile bool _serving;

    public override void _EnterTree()
    {
        try
        {
            Acquire();
        }
        catch (Exception e)
        {
            // Fail-safe: never let the single-instance machinery wedge the game.
            GD.PushWarning($"SingleInstance: acquire failed, running without guard: {e.Message}");
        }
    }

    public override void _ExitTree()
    {
        _serving = false;
        try { _listener?.Stop(); } catch { /* already stopped */ }
        _listener = null;

        // Only the claim-winner deletes its file on a clean exit — a duplicate
        // that quit must never remove the live holder's file. A crash/kill skips
        // _ExitTree entirely, leaving the file for the probe to self-correct.
        if (_ownsFile && _filePath != null)
            TryDelete(_filePath);
    }

    private void Acquire()
    {
        var dir = ResolveDataDir();
        Directory.CreateDirectory(dir);
        _filePath = Path.Combine(dir, FileName);

        // Bind FIRST (mirrors the launcher): TcpListener.Start() begins listening
        // immediately, so by the time the discovery file exists below we already
        // answer connects — a second starter that sees the file always gets an
        // answer and can never mistake an initialising instance for a stale one.
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        int port = ((IPEndPoint)listener.LocalEndpoint).Port;

        for (int attempt = 0; attempt < AcquireAttempts; attempt++)
        {
            try
            {
                using (var fs = new FileStream(_filePath, FileMode.CreateNew, FileAccess.Write, FileShare.Read))
                using (var w = new StreamWriter(fs, new UTF8Encoding(false)))
                {
                    w.Write($"{{\"port\":{port},\"proto\":{Proto}}}");
                }
                // Claimed the slot — start serving the banner for the process life.
                _listener = listener;
                _ownsFile = true;
                _serving = true;
                StartAcceptLoop(listener);
                GD.Print($"SingleInstance: acquired game slot on 127.0.0.1:{port}.");
                return;
            }
            catch (IOException) when (File.Exists(_filePath))
            {
                // The file already exists — live game, or a stale leftover?
                int? otherPort = ReadPortWithRetry(_filePath);
                if (otherPort is int p && ConnectBannerMatches(p))
                {
                    // A real game is live → we are a duplicate → quit cleanly
                    // before the main scene does anything meaningful.
                    IsDuplicate = true;
                    listener.Stop();
                    GD.Print("SingleInstance: another game instance is live — exiting.");
                    GetTree().CallDeferred(SceneTree.MethodName.Quit);
                    return;
                }
                // Stale (crashed game, recycled port, stranger, or no port) → reclaim.
                GD.Print("SingleInstance: stale game_instance.json — reclaiming.");
                TryDelete(_filePath);
            }
        }

        // Exhausted attempts → fail-safe: run normally without a claim.
        listener.Stop();
        GD.PushWarning("SingleInstance: exhausted claim attempts — running without guard.");
    }

    /// <summary>Background thread that owns the listener for the process lifetime
    /// and writes the role banner on every accepted connection. On exit
    /// <see cref="_ExitTree"/> stops the listener, which unblocks the accept call
    /// with an exception we treat as a normal shutdown.</summary>
    private void StartAcceptLoop(TcpListener listener)
    {
        byte[] banner = Encoding.ASCII.GetBytes($"BRISKA-BLAST\t{Proto}\t{Role}\n");
        var thread = new Thread(() =>
        {
            try
            {
                while (_serving)
                {
                    using var client = listener.AcceptTcpClient();
                    try
                    {
                        using var stream = client.GetStream();
                        stream.Write(banner, 0, banner.Length);
                        stream.Flush();
                    }
                    catch { /* per-connection error: keep serving */ }
                }
            }
            catch
            {
                // listener.Stop() during shutdown unblocks AcceptTcpClient — expected.
            }
        })
        {
            IsBackground = true,
            Name = "briska-singleinstance",
        };
        thread.Start();
    }

    /// <summary>Connect to a local port and check it answers with the GAME
    /// banner within the timeout. Any failure → false. Used to tell a live game
    /// from a stale file (and to reject a stranger that recycled the port).</summary>
    private static bool ConnectBannerMatches(int port)
    {
        try
        {
            using var client = new TcpClient();
            if (!client.ConnectAsync(IPAddress.Loopback, port).Wait(ProbeTimeoutMs))
                return false;
            client.ReceiveTimeout = ProbeTimeoutMs;
            using var stream = client.GetStream();

            byte[] expected = Encoding.ASCII.GetBytes($"BRISKA-BLAST\t{Proto}\t{Role}\n");
            byte[] buf = new byte[expected.Length];
            int total = 0;
            while (total < expected.Length)
            {
                int n = stream.Read(buf, total, expected.Length - total);
                if (n <= 0) break;
                total += n;
            }
            if (total < expected.Length) return false;
            for (int i = 0; i < expected.Length; i++)
                if (buf[i] != expected[i]) return false;
            return true;
        }
        catch
        {
            return false;
        }
    }

    private static int? ReadPortWithRetry(string path)
    {
        for (int attempt = 0; attempt < PortReadAttempts; attempt++)
        {
            int? p = ReadPort(path);
            if (p is int) return p;
            if (attempt + 1 < PortReadAttempts)
                Thread.Sleep(PortReadBackoffMs);
        }
        return null;
    }

    private static int? ReadPort(string path)
    {
        try
        {
            using var doc = JsonDocument.Parse(File.ReadAllText(path));
            if (doc.RootElement.TryGetProperty("port", out var portEl) &&
                portEl.TryGetInt32(out int port))
                return port;
        }
        catch { /* missing / partial / corrupt → no port */ }
        return null;
    }

    /// <summary>Locate the per-user data dir: the launcher handoff's value when
    /// present, else the same dir computed locally (editor / standalone run).</summary>
    private static string ResolveDataDir()
    {
        var handoff = LaunchArgs.FromLauncher;
        if (!string.IsNullOrEmpty(handoff?.DataDir))
            return handoff!.DataDir!;
        return FallbackDataDir();
    }

    /// <summary>Mirrors the Rust <c>directories</c> crate
    /// (<c>ProjectDirs::from("", "", "BriskaBlast").data_dir()</c>) exactly, so a
    /// standalone game writes where a launcher would probe. Note the per-platform
    /// asymmetry: Windows/macOS append <c>/data</c>; Linux lowercases and has no
    /// <c>/data</c> suffix. (A standalone game and a launcher-spawned game running
    /// at once is an unsupported edge — online play is impossible standalone — but
    /// both resolve the same path regardless, so they still agree.)</summary>
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

    private static void TryDelete(string path)
    {
        try { File.Delete(path); }
        catch (Exception e)
        {
            GD.PushWarning($"SingleInstance: failed to delete '{path}': {e.Message}");
        }
    }
}
