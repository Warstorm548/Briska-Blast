using System.Collections.Generic;
using Godot;
using BriskaBlast.Core;
using BriskaBlast.UI.Menus;

namespace BriskaBlast.Game;

/// <summary>End of match: freeze the sim behind the end screen and hand navigation
/// to it. Entered from <see cref="MatchFlow"/>'s game-over relay.</summary>
public partial class GameScene
{
    // ---- end-of-match (MatchFlow's GameOver relay) ----

    // End-game overlay, shown on the server's GameOver. Once `_gameOver` is set the
    // simulation is frozen and the EndGameMenu owns navigation; the following
    // SessionEnded teardown is ignored so it can't yank the player off the board.
    private EndGameMenu? _endGameMenu;
    private bool _gameOver;

    /// <summary>A player met the win condition. Freeze the sim, clear the pause
    /// overlays and chat's hold on the keyboard, then put the end screen up and
    /// give the cursor back for its buttons. Idempotent — a second relay of the
    /// same result finds <c>_gameOver</c> already set.</summary>
    private void OnGameOver(string winnerPlayerId, Dictionary<string, int> scores)
    {
        if (_gameOver)
            return;
        _gameOver = true;

        // Adopt the server's final tally so the frozen board and the leaderboard
        // are exact even if the preceding ScoreUpdate was missed.
        _state.ApplyScores(scores);
        _view.Render(_state); // one last paint, then the sim freezes (_PhysicsProcess early-returns)
        _leaderboard.SyncFrom(_state);

        // The match is over: drop the pause overlays if they were open (the Esc
        // menu and a pause-on-rejoin hold alike — the end screen supersedes
        // both), then show it on top of the frozen game.
        if (_pauseMenu != null)
            ClosePauseMenu();
        // Same for chat: the end screen takes the keyboard for its own buttons,
        // so the input must not still be holding it.
        _chat.ReleaseInput();
        _flowPaused = false;
        _resumeCountdownActive = false;
        RemovePausePanel();

        _endGameMenu = GD.Load<PackedScene>("res://src/ui/menus/EndGameMenu.tscn")
            .Instantiate<EndGameMenu>();
        _endGameMenu.MainMenuRequested += OnEndGameMainMenu;
        _endGameMenu.HostRequested += OnEndGameHost;
        AddChild(_endGameMenu);
        _endGameMenu.Populate(winnerPlayerId, scores);
        // Play is over and the end screen is a menu: the pointer comes back. Last,
        // because the rule reads _endGameMenu and the ClosePauseMenu above already
        // ran it once while this was still null.
        UpdateCursor();
    }

    // "Return to Main Menu": the one MatchFlow teardown (idempotent — a second
    // press or a racing lifecycle event finds the flow already Idle).
    private void OnEndGameMainMenu() => MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);

    // "Host Game": tear the finished session down, then go set up a new one.
    private void OnEndGameHost() =>
        MatchFlow.Instance.EndMatchTo("res://src/ui/menus/HostSetupMenu.tscn");
}
