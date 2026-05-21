using Godot;

namespace BriskaBlast.Core;

public static class GameVersion
{
    public const string Fallback = "0.0.0";

    private static string? _cached;

    public static string Current => _cached ??= Read();

    private static string Read()
    {
        if (!ProjectSettings.HasSetting("application/config/version"))
            return Fallback;
        return ProjectSettings.GetSetting("application/config/version").AsString();
    }
}
