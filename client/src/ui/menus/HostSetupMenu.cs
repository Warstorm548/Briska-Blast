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
	private HSlider _splitterInterval = null!;
	private Label _splitterIntervalValue = null!;
	private CheckBox _chainSplit = null!;
	private HSlider _lootDropInterval = null!;
	private Label _lootDropIntervalValue = null!;
	private CheckBox _barrierEnabled = null!;
	private HSlider _barrierWeight = null!;
	private Label _barrierWeightValue = null!;
	private HSlider _barrierDuration = null!;
	private Label _barrierDurationValue = null!;
	private Label _lootTableSummary = null!;

	// The host has subscribed more than 100% across the loot table, which is
	// impossible to satisfy. Blocks Create so the UI refuses before the server has to.
	private bool _lootOverSubscribed;
	private bool _busy;
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

		// Advanced "Random Spawns": BallSpliter cadence + chain-split. Bounds come
		// from the shared SpawnSettings constants so the slider and the server check
		// can't drift; defaults match a host who never opens this tab.
		_splitterInterval = GetNode<HSlider>("%SplitterInterval");
		_splitterIntervalValue = GetNode<Label>("%SplitterIntervalValue");
		_chainSplit = GetNode<CheckBox>("%ChainSplit");
		_splitterInterval.MinValue = SpawnSettingsDto.IntervalMin;
		_splitterInterval.MaxValue = SpawnSettingsDto.IntervalMax;
		_splitterInterval.Value = SpawnSettingsDto.IntervalDefault;
		_chainSplit.ButtonPressed = SpawnSettingsDto.ChainSplitDefault;
		_splitterInterval.ValueChanged += _ => UpdateSplitterLabel();
		UpdateSplitterLabel();

		// Loot Table tab: drop cadence + the per-item enable / weight / duration.
		// Bounds come from the shared LootSettings constants for the same reason as
		// above — the scene's authored min/max values are decorative.
		_lootDropInterval = GetNode<HSlider>("%LootDropInterval");
		_lootDropIntervalValue = GetNode<Label>("%LootDropIntervalValue");
		_barrierEnabled = GetNode<CheckBox>("%BarrierEnabled");
		_barrierWeight = GetNode<HSlider>("%BarrierWeight");
		_barrierWeightValue = GetNode<Label>("%BarrierWeightValue");
		_barrierDuration = GetNode<HSlider>("%BarrierDuration");
		_barrierDurationValue = GetNode<Label>("%BarrierDurationValue");
		_lootTableSummary = GetNode<Label>("%LootTableSummary");

		_lootDropInterval.MinValue = LootSettingsDto.IntervalMin;
		_lootDropInterval.MaxValue = LootSettingsDto.IntervalMax;
		_lootDropInterval.Value = LootSettingsDto.IntervalDefault;
		_barrierEnabled.ButtonPressed = LootSettingsDto.BarrierEnabledDefault;
		_barrierWeight.MinValue = LootSettingsDto.WeightMin;
		_barrierWeight.MaxValue = LootSettingsDto.WeightMax;
		_barrierWeight.Value = LootSettingsDto.BarrierWeightDefault;
		_barrierDuration.MinValue = LootSettingsDto.BarrierDurationMin;
		_barrierDuration.MaxValue = LootSettingsDto.BarrierDurationMax;
		_barrierDuration.Value = LootSettingsDto.BarrierDurationDefault;

		_lootDropInterval.ValueChanged += _ => UpdateLootLabels();
		_barrierWeight.ValueChanged += _ => UpdateLootLabels();
		_barrierDuration.ValueChanged += _ => UpdateLootLabels();
		_barrierEnabled.Toggled += _ => UpdateLootLabels();
		UpdateLootLabels();

		// Tab titles come from the child node names unless set here, which is why
		// they otherwise read "BasicSetup". Naming all three keeps them consistent.
		var tabs = GetNode<TabContainer>("%TabContainer");
		tabs.SetTabTitle(0, "Basic Setup");
		tabs.SetTabTitle(1, "Advanced");
		tabs.SetTabTitle(2, "Loot Table");

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

	// The "every Ns" readout to the right of the splitter slider.
	private void UpdateSplitterLabel() =>
		_splitterIntervalValue.Text = $"every {(int)_splitterInterval.Value}s";

	// The readouts beside the loot sliders, plus the live resolved-rate summary.
	private void UpdateLootLabels()
	{
		_lootDropIntervalValue.Text = $"every {(int)_lootDropInterval.Value}s";
		_barrierWeightValue.Text = $"{(int)_barrierWeight.Value}";
		_barrierDurationValue.Text = $"{(int)_barrierDuration.Value}s";
		UpdateLootSummary();
	}

	/// <summary>Show each item's RESOLVED drop rate rather than its raw weight, plus
	/// the chance of no drop at all.
	///
	/// Showing resolved rates is the point: a weight stops being a literal percentage
	/// the moment a second item shares it (two items on 50 subscribe one 50 and take
	/// 25% each), so raw weights would quietly mislead. This way a host sees the split
	/// happen while setting it, not mid-match.</summary>
	private void UpdateLootSummary()
	{
		var settings = BuildLootSettings();
		var rates = settings.ResolvedRates();

		var text = new System.Text.StringBuilder();
		for (int i = 0; i < rates.Length; i++)
		{
			if (rates[i] <= 0f)
				continue;
			if (text.Length > 0)
				text.Append("   ·   ");
			var item = BriskaBlast.Game.ItemRegistry.LootOrder[i];
			text.Append($"{BriskaBlast.Game.ItemRegistry.DisplayName(item)} {Format(rates[i])}%");
		}

		int total = settings.SubscribedTotal();
		_lootOverSubscribed = total > LootSettingsDto.WeightTotalMax;
		if (_lootOverSubscribed)
		{
			// Unreachable with a single item (its own slider caps at 100); this is
			// what will catch an oversubscribed table once item #2 exists.
			_lootTableSummary.Text =
				$"Drop chances total {total}% — that is more than 100%. Lower one to continue.";
			_lootTableSummary.AddThemeColorOverride("font_color", new Color(1f, 0.45f, 0.45f));
		}
		else
		{
			if (text.Length > 0)
				text.Append("   ·   ");
			text.Append($"Nothing {Format(settings.NothingRate())}%");
			_lootTableSummary.Text = text.Length > 0
				? text.ToString()
				: "No items enabled — nothing will ever drop.";
			_lootTableSummary.AddThemeColorOverride("font_color", new Color(0.7f, 0.78f, 0.9f));
		}

		RefreshCreateEnabled();

		// Whole numbers stay whole; a split bucket shows its half.
		static string Format(float v) =>
			Mathf.IsEqualApprox(v, Mathf.Round(v)) ? $"{Mathf.RoundToInt(v)}" : $"{v:0.#}";
	}

	private void RefreshCreateEnabled() => _createButton.Disabled = _busy || _lootOverSubscribed;

	private LootSettingsDto BuildLootSettings()
	{
		// Clamp defensively to the shared ranges (the sliders already bound these;
		// the server re-checks regardless).
		int interval = Math.Clamp((int)_lootDropInterval.Value, LootSettingsDto.IntervalMin, LootSettingsDto.IntervalMax);
		int weight = Math.Clamp((int)_barrierWeight.Value, LootSettingsDto.WeightMin, LootSettingsDto.WeightMax);
		int duration = Math.Clamp((int)_barrierDuration.Value, LootSettingsDto.BarrierDurationMin, LootSettingsDto.BarrierDurationMax);
		return new LootSettingsDto(interval, _barrierEnabled.ButtonPressed, weight, duration);
	}

	private SpawnSettingsDto BuildSpawnSettings()
	{
		// Clamp defensively to the shared range (the slider already bounds it; the
		// server re-checks regardless).
		int interval = Math.Clamp((int)_splitterInterval.Value, SpawnSettingsDto.IntervalMin, SpawnSettingsDto.IntervalMax);
		return new SpawnSettingsDto(interval, _chainSplit.ButtonPressed);
	}

	// Read UI values on the main thread, then do the async server round-trip.
	// The awaited continuation is not guaranteed to be on the main thread, so
	// every result handler is marshalled back via Callable.From(...).CallDeferred().
	private async void OnCreatePressed()
	{
		var mode = Modes[_gameMode.Selected];
		var max = (int)_maxPlayers.Value;
		var winCondition = BuildWinCondition();
		var spawnSettings = BuildSpawnSettings();
		var lootSettings = BuildLootSettings();

		SetBusy(true, "Creating session…");

		var ctx = SessionContext.Instance;
		if (!await ctx.EnsureIdentityAsync())
		{
			Callable.From(() => SetBusy(false, "No identity — launch via the launcher.")).CallDeferred();
			return;
		}

		var result = await ctx.Api.HostAsync(ctx.PlayerId, ctx.SecretToken, mode.WireName, max,
			winCondition, spawnSettings, lootSettings);
		Callable.From(() => OnHostComplete(result, mode.DisplayName, max, winCondition, spawnSettings, lootSettings)).CallDeferred();
	}

	private void OnHostComplete(ApiResult<HostResponse> result, string modeDisplay, int max,
		WinConditionDto winCondition, SpawnSettingsDto spawnSettings, LootSettingsDto lootSettings)
	{
		if (result.Ok && result.Value is { } r)
		{
			SessionContext.Instance.StartHostSession(r.SessionCode, modeDisplay, max, winCondition,
				spawnSettings, lootSettings);
			GetTree().ChangeSceneToFile("res://src/ui/menus/SessionLobby.tscn");
			return;
		}

		// Surface the trust-boundary rejections in plain language; fall back to the
		// raw error code for anything unexpected.
		string message = result.ErrorCode switch
		{
			"invalid_win_condition" => $"Score must be {WinConditionDto.ScoreMin}–{WinConditionDto.ScoreMax}.",
			"invalid_spawn_settings" => $"Splitter interval must be {SpawnSettingsDto.IntervalMin}–{SpawnSettingsDto.IntervalMax}s.",
			"invalid_loot_settings" => "A loot table setting is out of range.",
			"invalid_player_count" => "That player count isn't allowed for this mode.",
			_ => $"Could not host: {result.ErrorCode}",
		};
		SetBusy(false, message);
	}

	private void SetBusy(bool busy, string message)
	{
		_busy = busy;
		// Not a plain assignment: an oversubscribed loot table also disables Create,
		// and clearing busy must not re-enable it.
		RefreshCreateEnabled();
		_status.Text = message;
		_status.Visible = !string.IsNullOrEmpty(message);
	}
}
