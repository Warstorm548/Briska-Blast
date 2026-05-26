using Godot;
using BriskaBlast.Core;
using System.Collections.Generic;

namespace BriskaBlast.UI.Menus;

public partial class SettingsMenu : Control
{
    private static readonly (string Label, string Key)[] WindowModes =
    {
        ("Windowed", "windowed"),
        ("Fullscreen", "fullscreen"),
        ("Exclusive Fullscreen", "exclusive"),
    };

    private static readonly (string Label, string Key)[] VSyncModes =
    {
        ("Disabled", "disabled"),
        ("Enabled", "enabled"),
        ("Adaptive", "adaptive"),
        ("Mailbox", "mailbox"),
    };

    private static readonly (string Label, int Value)[] FpsCaps =
    {
        ("Unlimited", 0),
        ("30", 30),
        ("60", 60),
        ("120", 120),
        ("144", 144),
        ("240", 240),
    };

    private static readonly (string Label, string Key)[] Renderers =
    {
        ("Forward+", "forward_plus"),
        ("Mobile", "mobile"),
        ("Compatibility", "compatibility"),
    };

    private static readonly (string Label, int Value)[] MsaaLevels =
    {
        ("Disabled", 0),
        ("2x", 2),
        ("4x", 4),
        ("8x", 8),
    };

    private static readonly (string Label, string Key)[] TextureFilters =
    {
        ("Nearest", "nearest"),
        ("Linear", "linear"),
        ("Anisotropic 4x", "aniso4"),
        ("Anisotropic 8x", "aniso8"),
        ("Anisotropic 16x", "aniso16"),
    };

    private static readonly (string Label, string Key)[] ShadowQualities =
    {
        ("Off", "off"),
        ("Low", "low"),
        ("Medium", "medium"),
        ("High", "high"),
    };

    private static readonly (string Label, string Key)[] TextSizes =
    {
        ("Small", "small"),
        ("Medium", "medium"),
        ("Large", "large"),
    };

    private static readonly Vector2I[] CommonResolutions =
    {
        new(1280, 720),
        new(1366, 768),
        new(1600, 900),
        new(1920, 1080),
        new(2560, 1440),
        new(3840, 2160),
    };

    private OptionButton _windowMode = null!;
    private OptionButton _resolution = null!;
    private HSlider _uiScale = null!;
    private Label _uiScaleValue = null!;
    private OptionButton _vsync = null!;
    private OptionButton _fpsCap = null!;
    private OptionButton _renderer = null!;
    private OptionButton _msaa = null!;
    private OptionButton _textureFilter = null!;
    private OptionButton _shadows = null!;
    private CheckBox _postFx = null!;
    private HSlider _master = null!;
    private Label _masterValue = null!;
    private HSlider _music = null!;
    private Label _musicValue = null!;
    private HSlider _sfx = null!;
    private Label _sfxValue = null!;
    private HSlider _uiVol = null!;
    private Label _uiVolValue = null!;
    private OptionButton _outputDevice = null!;
    private CheckBox _showFps = null!;
    private CheckBox _showPing = null!;
    private HSlider _hudScale = null!;
    private Label _hudScaleValue = null!;
    private OptionButton _textSize = null!;

    private Vector2I[] _availableResolutions = System.Array.Empty<Vector2I>();

    public override void _Ready()
    {
        BindNodes();
        PopulateOptions();
        LoadFromSettings();
        WireSignals();
    }

    private void BindNodes()
    {
        _windowMode = GetNode<OptionButton>("%WindowMode");
        _resolution = GetNode<OptionButton>("%Resolution");
        _uiScale = GetNode<HSlider>("%UIScale");
        _uiScaleValue = GetNode<Label>("%UIScaleValue");
        _vsync = GetNode<OptionButton>("%VSync");
        _fpsCap = GetNode<OptionButton>("%FpsCap");
        _renderer = GetNode<OptionButton>("%Renderer");
        _msaa = GetNode<OptionButton>("%Msaa");
        _textureFilter = GetNode<OptionButton>("%TextureFilter");
        _shadows = GetNode<OptionButton>("%Shadows");
        _postFx = GetNode<CheckBox>("%PostFX");
        _master = GetNode<HSlider>("%Master");
        _masterValue = GetNode<Label>("%MasterValue");
        _music = GetNode<HSlider>("%Music");
        _musicValue = GetNode<Label>("%MusicValue");
        _sfx = GetNode<HSlider>("%Sfx");
        _sfxValue = GetNode<Label>("%SfxValue");
        _uiVol = GetNode<HSlider>("%UIVol");
        _uiVolValue = GetNode<Label>("%UIVolValue");
        _outputDevice = GetNode<OptionButton>("%OutputDevice");
        _showFps = GetNode<CheckBox>("%ShowFps");
        _showPing = GetNode<CheckBox>("%ShowPing");
        _hudScale = GetNode<HSlider>("%HudScale");
        _hudScaleValue = GetNode<Label>("%HudScaleValue");
        _textSize = GetNode<OptionButton>("%TextSize");
    }

    private void PopulateOptions()
    {
        foreach (var (label, _) in WindowModes) _windowMode.AddItem(label);
        foreach (var (label, _) in VSyncModes) _vsync.AddItem(label);
        foreach (var (label, _) in FpsCaps) _fpsCap.AddItem(label);
        foreach (var (label, _) in Renderers) _renderer.AddItem(label);
        foreach (var (label, _) in MsaaLevels) _msaa.AddItem(label);
        foreach (var (label, _) in TextureFilters) _textureFilter.AddItem(label);
        foreach (var (label, _) in ShadowQualities) _shadows.AddItem(label);
        foreach (var (label, _) in TextSizes) _textSize.AddItem(label);

        var screenSize = DisplayServer.ScreenGetSize();
        var avail = new List<Vector2I>();
        foreach (var res in CommonResolutions)
        {
            if (res.X <= screenSize.X && res.Y <= screenSize.Y)
                avail.Add(res);
        }
        _availableResolutions = avail.ToArray();
        foreach (var res in _availableResolutions)
            _resolution.AddItem($"{res.X} x {res.Y}");

        foreach (var dev in AudioServer.GetOutputDeviceList())
            _outputDevice.AddItem(dev);
        if (_outputDevice.ItemCount == 0)
            _outputDevice.AddItem("Default");
    }

    private void LoadFromSettings()
    {
        var s = SettingsManager.Instance;

        SelectByKey(_windowMode, WindowModes, s.GetString(SettingsManager.Display, "window_mode", "windowed"));
        var resX = s.GetInt(SettingsManager.Display, "resolution_x", 1280);
        var resY = s.GetInt(SettingsManager.Display, "resolution_y", 720);
        SelectResolution(resX, resY);
        _uiScale.Value = s.GetFloat(SettingsManager.Display, "ui_scale", 1.0f);
        UpdateScaleLabels();
        SelectByKey(_vsync, VSyncModes, s.GetString(SettingsManager.Display, "vsync", "enabled"));
        SelectByValue(_fpsCap, FpsCaps, s.GetInt(SettingsManager.Display, "fps_cap", 0));
        SelectByKey(_renderer, Renderers, s.GetString(SettingsManager.Display, "renderer", "forward_plus"));

        SelectByValue(_msaa, MsaaLevels, s.GetInt(SettingsManager.Graphics, "msaa", 0));
        SelectByKey(_textureFilter, TextureFilters, s.GetString(SettingsManager.Graphics, "texture_filter", "linear"));
        SelectByKey(_shadows, ShadowQualities, s.GetString(SettingsManager.Graphics, "shadows", "medium"));
        _postFx.ButtonPressed = s.GetBool(SettingsManager.Graphics, "post_fx", true);

        _master.Value = s.GetInt(SettingsManager.Audio, "master_vol", 80);
        _music.Value = s.GetInt(SettingsManager.Audio, "music_vol", 70);
        _sfx.Value = s.GetInt(SettingsManager.Audio, "sfx_vol", 80);
        _uiVol.Value = s.GetInt(SettingsManager.Audio, "ui_vol", 70);
        UpdateAudioLabels();
        SelectOutputDevice(s.GetString(SettingsManager.Audio, "output_device", "Default"));

        _showFps.ButtonPressed = s.GetBool(SettingsManager.Gameplay, "show_fps", false);
        _showPing.ButtonPressed = s.GetBool(SettingsManager.Gameplay, "show_ping", false);
        _hudScale.Value = s.GetFloat(SettingsManager.Gameplay, "hud_scale", 1.0f);
        SelectByKey(_textSize, TextSizes, s.GetString(SettingsManager.Accessibility, "text_size", "medium"));
    }

    private void WireSignals()
    {
        _uiScale.ValueChanged += _ => UpdateScaleLabels();
        _hudScale.ValueChanged += _ => UpdateScaleLabels();
        _master.ValueChanged += _ => UpdateAudioLabels();
        _music.ValueChanged += _ => UpdateAudioLabels();
        _sfx.ValueChanged += _ => UpdateAudioLabels();
        _uiVol.ValueChanged += _ => UpdateAudioLabels();

        GetNode<Button>("%ApplyButton").Pressed += OnApplyPressed;
        GetNode<Button>("%ReturnButton").Pressed += () =>
            GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }

    private void UpdateScaleLabels()
    {
        _uiScaleValue.Text = $"{_uiScale.Value:F2}x";
        _hudScaleValue.Text = $"{_hudScale.Value:F2}x";
    }

    private void UpdateAudioLabels()
    {
        _masterValue.Text = ((int)_master.Value).ToString();
        _musicValue.Text = ((int)_music.Value).ToString();
        _sfxValue.Text = ((int)_sfx.Value).ToString();
        _uiVolValue.Text = ((int)_uiVol.Value).ToString();
    }

    private void OnApplyPressed()
    {
        var s = SettingsManager.Instance;

        s.Set(SettingsManager.Display, "window_mode", WindowModes[_windowMode.Selected].Key);
        if (_resolution.Selected >= 0 && _availableResolutions.Length > 0)
        {
            var res = _availableResolutions[_resolution.Selected];
            s.Set(SettingsManager.Display, "resolution_x", res.X);
            s.Set(SettingsManager.Display, "resolution_y", res.Y);
        }
        s.Set(SettingsManager.Display, "ui_scale", (float)_uiScale.Value);
        s.Set(SettingsManager.Display, "vsync", VSyncModes[_vsync.Selected].Key);
        s.Set(SettingsManager.Display, "fps_cap", FpsCaps[_fpsCap.Selected].Value);
        s.Set(SettingsManager.Display, "renderer", Renderers[_renderer.Selected].Key);

        s.Set(SettingsManager.Graphics, "msaa", MsaaLevels[_msaa.Selected].Value);
        s.Set(SettingsManager.Graphics, "texture_filter", TextureFilters[_textureFilter.Selected].Key);
        s.Set(SettingsManager.Graphics, "shadows", ShadowQualities[_shadows.Selected].Key);
        s.Set(SettingsManager.Graphics, "post_fx", _postFx.ButtonPressed);

        s.Set(SettingsManager.Audio, "master_vol", (int)_master.Value);
        s.Set(SettingsManager.Audio, "music_vol", (int)_music.Value);
        s.Set(SettingsManager.Audio, "sfx_vol", (int)_sfx.Value);
        s.Set(SettingsManager.Audio, "ui_vol", (int)_uiVol.Value);
        var deviceName = _outputDevice.Selected >= 0
            ? _outputDevice.GetItemText(_outputDevice.Selected)
            : "Default";
        s.Set(SettingsManager.Audio, "output_device", deviceName);

        s.Set(SettingsManager.Gameplay, "show_fps", _showFps.ButtonPressed);
        s.Set(SettingsManager.Gameplay, "show_ping", _showPing.ButtonPressed);
        s.Set(SettingsManager.Gameplay, "hud_scale", (float)_hudScale.Value);
        s.Set(SettingsManager.Accessibility, "text_size", TextSizes[_textSize.Selected].Key);

        s.Save();
        s.ApplyDisplay();
        GD.Print("Settings applied & saved");
    }

    private static void SelectByKey(OptionButton btn, (string Label, string Key)[] table, string key)
    {
        for (int i = 0; i < table.Length; i++)
        {
            if (table[i].Key == key) { btn.Selected = i; return; }
        }
        btn.Selected = 0;
    }

    private static void SelectByValue(OptionButton btn, (string Label, int Value)[] table, int value)
    {
        for (int i = 0; i < table.Length; i++)
        {
            if (table[i].Value == value) { btn.Selected = i; return; }
        }
        btn.Selected = 0;
    }

    private void SelectResolution(int x, int y)
    {
        for (int i = 0; i < _availableResolutions.Length; i++)
        {
            if (_availableResolutions[i].X == x && _availableResolutions[i].Y == y)
            {
                _resolution.Selected = i;
                return;
            }
        }
        if (_availableResolutions.Length > 0) _resolution.Selected = 0;
    }

    private void SelectOutputDevice(string name)
    {
        for (int i = 0; i < _outputDevice.ItemCount; i++)
        {
            if (_outputDevice.GetItemText(i) == name) { _outputDevice.Selected = i; return; }
        }
        if (_outputDevice.ItemCount > 0) _outputDevice.Selected = 0;
    }
}
