using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>
/// Resolves the base URLs the client talks to. The host is
/// <b>compile-time baked per channel</b> via <see cref="BuildConfig"/> —
/// there is no runtime host selection, so channel isolation is enforced at
/// the build-artifact level and a build can only ever reach its own
/// channel's server.
/// </summary>
public static class ServerEndpoint
{
    /// <summary>e.g. <c>https://briskadev.phoenixwired.com</c></summary>
    public static string HttpBase { get; }

    /// <summary>e.g. <c>wss://briskadev.phoenixwired.com</c></summary>
    public static string WsBase { get; }

    static ServerEndpoint()
    {
        HttpBase = $"https://{BuildConfig.ServerHost}".TrimEnd('/');
        WsBase = HttpBase
            .Replace("https://", "wss://")
            .Replace("http://", "ws://");
    }
}
