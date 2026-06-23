using System;
using System.Collections.Generic;
using System.Linq;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// End-of-match overlay shown when the server signals <c>GameOver</c>. Styled like
/// <see cref="PauseMenu"/> (the game is visible, dimmed, behind it) but the match
/// has truly ended — the owning <see cref="Game.GameScene"/> freezes the
/// simulation while this is up. Purely a view: it surfaces the player's choice via
/// events and lets the game scene perform the teardown / scene change. A
/// <see cref="CanvasLayer"/> so it draws over the Node2D match.
/// </summary>
public partial class EndGameMenu : CanvasLayer
{
    /// <summary>Tear down the session and return to the main menu.</summary>
    public event Action? MainMenuRequested;
    /// <summary>Tear down the session and go set up a new hosted game.</summary>
    public event Action? HostRequested;

    public override void _Ready()
    {
        GetNode<Button>("%MainMenuButton").Pressed += () => MainMenuRequested?.Invoke();
        GetNode<Button>("%HostButton").Pressed += () => HostRequested?.Invoke();

        // Land focus on the safe default so a keyboard player isn't on an action
        // by surprise.
        GetNode<Button>("%MainMenuButton").GrabFocus();
    }

    /// <summary>Fill in the winner banner and the final-standings leaderboard.
    /// Lists <em>every</em> player who was in the session (from the frozen seating
    /// roster, falling back to the live roster), so a player who never scored still
    /// appears with 0. Sorted by score descending; the winner's row is highlighted.
    /// Names come from <see cref="SessionContext.DisplayNameFor"/>.</summary>
    public void Populate(string winnerPlayerId, IReadOnlyDictionary<string, int> scores)
    {
        var ctx = SessionContext.Instance;

        if (ctx != null)
            GetNode<Label>("%Winner").Text = $"{ctx.DisplayNameFor(winnerPlayerId)} wins!";

        var grid = GetNode<GridContainer>("%Leaderboard");
        foreach (var child in grid.GetChildren())
            child.QueueFree();

        // Full roster: prefer the frozen seating roster (self-inclusive), fall back
        // to the live lobby roster. De-dup defensively.
        var roster = new List<string>();
        if (ctx != null)
        {
            roster.AddRange(ctx.SeatOrder.Count > 0 ? ctx.SeatOrder : ctx.PlayerIds);
            foreach (var pid in scores.Keys) // include anyone with points but missing from the roster
                if (!roster.Contains(pid))
                    roster.Add(pid);
        }

        var goldColor = new Color(1f, 0.85f, 0.4f);
        var rows = roster
            .Distinct()
            .Select(pid => (Id: pid, Score: scores.TryGetValue(pid, out var s) ? s : 0))
            .OrderByDescending(r => r.Score)
            .ThenBy(r => ctx?.DisplayNameFor(r.Id) ?? r.Id, StringComparer.OrdinalIgnoreCase);

        foreach (var (id, score) in rows)
        {
            bool isWinner = id == winnerPlayerId;
            var nameLabel = new Label { Text = ctx?.DisplayNameFor(id) ?? id };
            var scoreLabel = new Label
            {
                Text = score.ToString(),
                HorizontalAlignment = HorizontalAlignment.Right,
            };
            scoreLabel.SizeFlagsHorizontal = Control.SizeFlags.ExpandFill;
            if (isWinner)
            {
                nameLabel.AddThemeColorOverride("font_color", goldColor);
                scoreLabel.AddThemeColorOverride("font_color", goldColor);
            }
            grid.AddChild(nameLabel);
            grid.AddChild(scoreLabel);
        }
    }
}
