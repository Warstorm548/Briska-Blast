using Godot;
using System;
using BriskaBlast.Core;
using BriskaBlast.Net;

namespace BriskaBlast.UI.Menus;

public partial class HostSetupMenu : Control
{
	private readonly record struct GameMode(string DisplayName, string WireName, int MinPlayers, int MaxPlayers);

	private static readonly GameMode[] Modes =
	{
		new("Extended", "extended", 2, 4),
	};

	private OptionButton _gameMode = null!;
	private SpinBox _maxPlayers = null!;
	private Button _createButton = null!;
	private Label _status = null!;

	public override void _Ready()
	{
		_gameMode = GetNode<OptionButton>("%GameMode");
		_maxPlayers = GetNode<SpinBox>("%MaxPlayers");
		_createButton = GetNode<Button>("%CreateButton");

		_status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
		_createButton.GetParent().AddChild(_status);

		for (int i = 0; i < Modes.Length; i++)
			_gameMode.AddItem(Modes[i].DisplayName, i);
		_gameMode.Selected = 0;
		_gameMode.ItemSelected += idx => ApplyMode((int)idx);
		ApplyMode(0);

		_createButton.Pressed += OnCreatePressed;
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

	// Read UI values on the main thread, then do the async server round-trip.
	// The awaited continuation is not guaranteed to be on the main thread, so
	// every result handler is marshalled back via Callable.From(...).CallDeferred().
	private async void OnCreatePressed()
	{
		var mode = Modes[_gameMode.Selected];
		var max = (int)_maxPlayers.Value;

		SetBusy(true, "Creating session…");

		var ctx = SessionContext.Instance;
		if (!await ctx.EnsureIdentityAsync())
		{
			Callable.From(() => SetBusy(false, "No identity — launch via the launcher.")).CallDeferred();
			return;
		}

		var result = await ctx.Api.HostAsync(ctx.PlayerId, ctx.SecretToken, mode.WireName, max);
		Callable.From(() => OnHostComplete(result, mode.DisplayName, max)).CallDeferred();
	}

	private void OnHostComplete(ApiResult<HostResponse> result, string modeDisplay, int max)
	{
		if (result.Ok && result.Value is { } r)
		{
			SessionContext.Instance.StartHostSession(r.SessionCode, modeDisplay, max);
			GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
			return;
		}
		SetBusy(false, $"Could not host: {result.ErrorCode}");
	}

	private void SetBusy(bool busy, string message)
	{
		_createButton.Disabled = busy;
		_status.Text = message;
		_status.Visible = !string.IsNullOrEmpty(message);
	}
}
