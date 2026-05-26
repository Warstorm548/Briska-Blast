using Godot;

namespace BriskaBlast.Core;

public partial class SettingsManager : Node
{
    public static SettingsManager Instance { get; private set; } = null!;

    public const string Display = "display";
    public const string Graphics = "graphics";
    public const string Audio = "audio";
    public const string Controls = "controls";
    public const string Gameplay = "gameplay";
    public const string Accessibility = "accessibility";

    private const string ConfigPath = "user://settings.cfg";
    private readonly ConfigFile _config = new();

    public override void _Ready()
    {
        Instance = this;
        _config.Load(ConfigPath);
        ApplyDisplay();
    }

    public int GetInt(string section, string key, int fallback)
        => (int)(long)_config.GetValue(section, key, fallback);

    public float GetFloat(string section, string key, float fallback)
        => (float)(double)_config.GetValue(section, key, fallback);

    public string GetString(string section, string key, string fallback)
        => (string)_config.GetValue(section, key, fallback);

    public bool GetBool(string section, string key, bool fallback)
        => (bool)_config.GetValue(section, key, fallback);

    public void Set(string section, string key, Variant value)
        => _config.SetValue(section, key, value);

    public Error Save() => _config.Save(ConfigPath);

    public void ApplyDisplay()
    {
        var mode = GetString(Display, "window_mode", "windowed");
        DisplayServer.WindowSetMode(mode switch
        {
            "fullscreen" => DisplayServer.WindowMode.Fullscreen,
            "exclusive" => DisplayServer.WindowMode.ExclusiveFullscreen,
            _ => DisplayServer.WindowMode.Windowed,
        });

        if (mode == "windowed")
        {
            var w = GetInt(Display, "resolution_x", 1280);
            var h = GetInt(Display, "resolution_y", 720);
            DisplayServer.WindowSetSize(new Vector2I(w, h));
        }

        var scale = GetFloat(Display, "ui_scale", 1.0f);
        GetWindow().ContentScaleFactor = scale;

        var vsync = GetString(Display, "vsync", "enabled");
        DisplayServer.WindowSetVsyncMode(vsync switch
        {
            "disabled" => DisplayServer.VSyncMode.Disabled,
            "adaptive" => DisplayServer.VSyncMode.Adaptive,
            "mailbox" => DisplayServer.VSyncMode.Mailbox,
            _ => DisplayServer.VSyncMode.Enabled,
        });

        Engine.MaxFps = GetInt(Display, "fps_cap", 0);
    }
}
