using Godot;
using System.Collections.Generic;

namespace BriskaBlast.Core;

public partial class SessionContext : Node
{
    public static SessionContext Instance { get; private set; } = null!;

    public string SessionCode { get; set; } = "";
    public string GameMode { get; set; } = "";
    public int MaxPlayers { get; set; }
    public List<string> PlayerNames { get; } = new();
    public int HostIndex { get; set; } = -1;
    public bool LocalPlayerIsHost { get; set; }

    public string LocalUsername { get; private set; } = "Player Username 1";

    public override void _Ready()
    {
        Instance = this;

        var handoff = LaunchArgs.FromLauncher;
        if (handoff?.Username is { Length: > 0 } name)
        {
            LocalUsername = name;
            GD.Print($"SessionContext: using launcher-supplied username '{name}'.");
        }
    }

    public void StartHostSession(string code, string mode, int maxPlayers)
    {
        SessionCode = code;
        GameMode = mode;
        MaxPlayers = maxPlayers;
        PlayerNames.Clear();
        PlayerNames.Add(LocalUsername);
        HostIndex = 0;
        LocalPlayerIsHost = true;
    }

    public void ClearSession()
    {
        SessionCode = "";
        GameMode = "";
        MaxPlayers = 0;
        PlayerNames.Clear();
        HostIndex = -1;
        LocalPlayerIsHost = false;
    }

    public void PromoteToHost(int index)
    {
        if (index < 0 || index >= PlayerNames.Count) return;
        HostIndex = index;
    }
}
