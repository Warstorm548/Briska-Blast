using Godot;
using System;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

public partial class HostSetupMenu : Control
{
	private readonly record struct GameMode(string DisplayName, int MinPlayers, int MaxPlayers);

	private static readonly GameMode[] Modes =
	{
		new("Extended", 2, 4),
	};

	private OptionButton _gameMode = null!;
	private SpinBox _maxPlayers = null!;

	public override void _Ready()
	{
		_gameMode = GetNode<OptionButton>("%GameMode");
		_maxPlayers = GetNode<SpinBox>("%MaxPlayers");

		for (int i = 0; i < Modes.Length; i++)
			_gameMode.AddItem(Modes[i].DisplayName, i);
		_gameMode.Selected = 0;
		_gameMode.ItemSelected += idx => ApplyMode((int)idx);
		ApplyMode(0);

		GetNode<Button>("%CreateButton").Pressed += OnCreatePressed;
		GetNode<Button>("%ReturnButton").Pressed += () =>
			GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
	}

	private void ApplyMode(int index)
	{
		var mode = Modes[index];
		_maxPlayers.MinValue = mode.MinPlayers;
		_maxPlayers.MaxValue = mode.MaxPlayers;
		_maxPlayers.Value = Math.Clamp((int)_maxPlayers.Value, mode.MinPlayers, mode.MaxPlayers);
	}

	private void OnCreatePressed()
	{
		var mode = Modes[_gameMode.Selected].DisplayName;
		var max = (int)_maxPlayers.Value;
		SessionContext.Instance.StartHostSession("ABC123", mode, max);
		GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
	}
}
