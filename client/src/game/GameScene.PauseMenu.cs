using Godot;
using BriskaBlast.Core;
using BriskaBlast.UI.Menus;

namespace BriskaBlast.Game;

/// <summary>The Esc-bound pause menu: a purely local overlay. The simulation keeps
/// running underneath and the other screens are unaffected — contrast the
/// server-driven freeze in GameScene.FlowPause.cs.</summary>
public partial class GameScene
{
    // ---- Esc pause menu ----

    // Esc-bound pause menu (the design mockup). Null while closed; while open the
    // match stays live underneath but local paddle/serve input is suspended.
    private PauseMenu? _pauseMenu;
    private bool _paused;

    /// <summary>Escape's action when chat is not holding the keyboard: open the
    /// pause menu, or close it if it is already up.</summary>
    private void TogglePauseMenu()
    {
        if (_pauseMenu != null)
            ClosePauseMenu();
        else
            OpenPauseMenu();
    }

    /// <summary>Put the in-match pause menu up. A local overlay only — the
    /// simulation keeps running underneath and the other screens are unaffected,
    /// unlike the flow pause a rejoin triggers.</summary>
    private void OpenPauseMenu()
    {
        // Only while actually playing — mid-leave (flow already Idle) or
        // post-match the scene is on its way out; don't pop a menu over it.
        if (MatchFlow.Instance.State != MatchFlowState.InMatch || _pauseMenu != null)
            return;
        // The menu grabs focus for its own buttons, so hand the keyboard back
        // first rather than leaving chat holding a latch it can no longer clear.
        _chat.ReleaseInput();
        _pauseMenu = GD.Load<PackedScene>("res://src/ui/menus/PauseMenu.tscn")
            .Instantiate<PauseMenu>();
        _pauseMenu.ReturnRequested += ClosePauseMenu;
        _pauseMenu.ExitToMenuRequested += OnExitToMenu;
        _pauseMenu.QuitRequested += OnQuitGame;
        AddChild(_pauseMenu);
        _paused = true;
        UpdateCursor();
    }

    // "Return to Session": just dismiss the overlay and resume play.
    private void ClosePauseMenu()
    {
        if (_pauseMenu == null)
            return;
        _pauseMenu.ReturnRequested -= ClosePauseMenu;
        _pauseMenu.ExitToMenuRequested -= OnExitToMenu;
        _pauseMenu.QuitRequested -= OnQuitGame;
        _pauseMenu.QueueFree();
        _pauseMenu = null;
        _paused = false;
        UpdateCursor();
    }

    // "Exit to main menu": leave WITHOUT an explicit `leave` frame, so the server
    // treats us as a transient drop — holding our slot for the 2-min reconnect
    // window and, if we were host, running the 30s promotion grace, exactly as if
    // we'd dropped (contrast a deliberate Leave, which would promote immediately).
    private void OnExitToMenu() => MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);

    // "Quit Game": same transient-drop teardown (peers keep our slot and run the
    // grace timers), then fully close the app. Even if the clean close doesn't
    // flush before exit, the dropped socket is still a non-`leave` disconnect, so
    // the server arms the same grace.
    private void OnQuitGame() => MatchFlow.Instance.QuitGame();
}
