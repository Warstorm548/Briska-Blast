using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>
/// Resolves the base URLs the client talks to. The host is
/// <b>compile-time baked per channel</b> via <see cref="BuildConfig"/> —
/// channel isolation is enforced at the build-artifact level, so a release
/// build can only ever reach its own channel's server.
///
/// In DEBUG/editor builds only, a <c>BRISKA_DEV_SERVER</c> environment
/// variable overrides the origin so a developer can point two editor
/// instances at a local server (e.g. <c>127.0.0.1:8080</c> or
/// <c>http://127.0.0.1:8080</c>). This override does not exist in release
/// builds, so it cannot weaken the channel-isolation guarantee.
/// </summary>
public static class ServerEndpoint
{
    /// <summary>e.g. <c>https://briskadev.phoenixwired.com</c></summary>
    public static string HttpBase { get; }

    /// <summary>e.g. <c>wss://briskadev.phoenixwired.com</c></summary>
    public static string WsBase { get; }

    static ServerEndpoint()
    {
        var origin = $"https://{BuildConfig.ServerHost}";

#if DEBUG
        var overrideOrigin = System.Environment.GetEnvironmentVariable("BRISKA_DEV_SERVER");
        if (!string.IsNullOrWhiteSpace(overrideOrigin))
        {
            // Accept either a bare authority ("host:port") or a full origin
            // ("http://host:port"). Bare authorities default to plaintext
            // http/ws since the override is for local dev.
            origin = overrideOrigin.Contains("://") ? overrideOrigin : $"http://{overrideOrigin}";
        }
#endif

        HttpBase = origin.TrimEnd('/');
        WsBase = HttpBase
            .Replace("https://", "wss://")
            .Replace("http://", "ws://");
    }
}
