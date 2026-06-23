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

	// Win-condition kinds for the advanced "Match Rules" dropdown. `HasScore`
	// drives whether the inline score SpinBox is shown. One entry for now; the
	// table is the seam for future kinds (and for mode-driven hiding later).
	private readonly record struct WinKind(string DisplayName, string WireName, bool HasScore);

	private static readonly WinKind[] WinKinds =
	{
		new("Set Score", WinConditionDto.SetScoreKind, true),
	};

	private OptionButton _gameMode = null!;
	private SpinBox _maxPlayers = null!;
	private OptionButton _winCondition = null!;
	private SpinBox _winScore = null!;
	private Label _winDescription = null!;
	private Button _createButton = null!;
	private Label _status = null!;

	public override void _Ready()
	{
		_gameMode = GetNode<OptionButton>("%GameMode");
		_maxPlayers = GetNode<SpinBox>("%MaxPlayers");
		_winCondition = GetNode<OptionButton>("%WinCondition");
		_winScore = GetNode<SpinBox>("%WinScore");
		_winDescription = GetNode<Label>("%WinDescription");
		_createButton = GetNode<Button>("%CreateButton");

		_status = new Label { HorizontalAlignment = HorizontalAlignment.Center, Visible = false };
		_createButton.GetParent().AddChild(_status);

		for (int i = 0; i < Modes.Length; i++)
			_gameMode.AddItem(Modes[i].DisplayName, i);
		_gameMode.Selected = 0;
		_gameMode.ItemSelected += idx => ApplyMode((int)idx);
		ApplyMode(0);

		// Advanced "Match Rules": win condition. Defaults to the first kind with
		// its SpinBox at the shared default, so a host who never opens this tab
		// still sends a valid default win condition.
		for (int i = 0; i < WinKinds.Length; i++)
			_winCondition.AddItem(WinKinds[i].DisplayName, i);
		_winCondition.Selected = 0;
		_winScore.MinValue = WinConditionDto.ScoreMin;
		_winScore.MaxValue = WinConditionDto.ScoreMax;
		_winScore.Value = WinConditionDto.ScoreDefault;
		_winCondition.ItemSelected += idx => ApplyWinKind((int)idx);
		_winScore.ValueChanged += _ => UpdateWinDescription();
		ApplyWinKind(0);

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

	// Show the score input only for kinds that take a target, then refresh the
	// live description. (Future kinds without a score hide the SpinBox.)
	private void ApplyWinKind(int index)
	{
		_winScore.Visible = WinKinds[index].HasScore;
		UpdateWinDescription();
	}

	// The "live" description to the right of the dropdown — reflects the current
	// selection and, for Set Score, the current target value.
	private void UpdateWinDescription()
	{
		var kind = WinKinds[_winCondition.Selected];
		_winDescription.Text = kind.HasScore
			? $"First player to reach {(int)_winScore.Value} points wins the match."
			: $"{kind.DisplayName} win condition.";
	}

	private WinConditionDto BuildWinCondition()
	{
		var kind = WinKinds[_winCondition.Selected];
		// Only Set Score carries a target today; clamp defensively to the shared
		// range (the SpinBox already bounds it, the server re-checks regardless).
		int target = Math.Clamp((int)_winScore.Value, WinConditionDto.ScoreMin, WinConditionDto.ScoreMax);
		return new WinConditionDto(kind.WireName, target);
	}

	// Read UI values on the main thread, then do the async server round-trip.
	// The awaited continuation is not guaranteed to be on the main thread, so
	// every result handler is marshalled back via Callable.From(...).CallDeferred().
	private async void OnCreatePressed()
	{
		var mode = Modes[_gameMode.Selected];
		var max = (int)_maxPlayers.Value;
		var winCondition = BuildWinCondition();

		SetBusy(true, "Creating session…");

		var ctx = SessionContext.Instance;
		if (!await ctx.EnsureIdentityAsync())
		{
			Callable.From(() => SetBusy(false, "No identity — launch via the launcher.")).CallDeferred();
			return;
		}

		var result = await ctx.Api.HostAsync(ctx.PlayerId, ctx.SecretToken, mode.WireName, max, winCondition);
		Callable.From(() => OnHostComplete(result, mode.DisplayName, max, winCondition)).CallDeferred();
	}

	private void OnHostComplete(ApiResult<HostResponse> result, string modeDisplay, int max, WinConditionDto winCondition)
	{
		if (result.Ok && result.Value is { } r)
		{
			SessionContext.Instance.StartHostSession(r.SessionCode, modeDisplay, max, winCondition);
			GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
			return;
		}

		// Surface the trust-boundary rejections in plain language; fall back to the
		// raw error code for anything unexpected.
		string message = result.ErrorCode switch
		{
			"invalid_win_condition" => $"Score must be {WinConditionDto.ScoreMin}–{WinConditionDto.ScoreMax}.",
			"invalid_player_count" => "That player count isn't allowed for this mode.",
			_ => $"Could not host: {result.ErrorCode}",
		};
		SetBusy(false, message);
	}

	private void SetBusy(bool busy, string message)
	{
		_createButton.Disabled = busy;
		_status.Text = message;
		_status.Visible = !string.IsNullOrEmpty(message);
	}
}
