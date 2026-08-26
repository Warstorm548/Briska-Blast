using System;
using System.Collections.Generic;
using BriskaBlast.Core;
using BriskaBlast.Game;
using Godot;

namespace BriskaBlast.UI;

/// <summary>
/// The in-match leaderboard, pinned top-left: one row per player, ranked by score,
/// over a translucent panel so the play field stays readable through it.
///
/// It replaced a single-line Label in <c>View2D</c> that listed players in
/// player_id order. That ordering was deliberate — it kept columns from moving —
/// and here the opposite is wanted: rows change places as the match does. The two
/// concessions that keeps it from becoming noise are that the order settles only
/// once every <see cref="ReorderIntervalMsec"/>, and that rows glide between
/// positions instead of snapping.
///
/// Sizes are fractions of VIEWPORT height, the same convention the action bar
/// uses and for the same reason: the logical viewport is only 2560×1440 at 16:9
/// and grows on one axis otherwise, so a fraction is the only thing that means
/// the same on every display. Geometry resolves once, in <see cref="_Ready"/> —
/// nothing in this client re-derives layout on a resize, deliberately.
/// </summary>
public partial class LeaderboardView : CanvasLayer
{
    /// <summary>How long a ranking stands before it is restated. Deliberately slow:
    /// re-sorting the instant a point lands turns a close match into a flicker, and
    /// the score itself already updates immediately, so nothing is being hidden.
    /// Tune here — it is the only place the cadence is expressed.</summary>
    private const ulong ReorderIntervalMsec = 3000;

    /// <summary>How long a row takes to travel to its new place. Long enough for the
    /// eye to follow a swap, short enough to finish well inside one reorder period.</summary>
    private const float SwapSeconds = 0.35f;

    private const float RowHeightHFrac = 44f / 1440f;
    private const float PanelWidthHFrac = 460f / 1440f;
    private const float PaddingHFrac = 14f / 1440f;
    private const float MarginHFrac = 16f / 1440f;

    /// <summary>Row text size, resolved from viewport height like every other
    /// dimension here so the type and the panel cannot drift apart.</summary>
    private const float FontHFrac = 28f / 1440f;

    /// <summary>The palette's neon accent — the same blue <c>MenuTheme.tres</c> uses
    /// for focus glow and the chat caret, so this panel reads as part of the same UI
    /// rather than a new colour arriving in the HUD.</summary>
    private static readonly Color NeonBlue = new(0.3f, 0.85f, 1f);

    /// <summary>Fill alpha. Low: this panel sits over live play, and the ball has to
    /// stay trackable behind it. Note this departs from the chat panel's dark navy
    /// fill — a light blue tint was the ask, so the fill is the accent, not the
    /// panel colour.</summary>
    private const float FillAlpha = 0.14f;

    private static readonly Color NameColor = new(0.93f, 0.96f, 1f);

    private Panel _panel = null!;

    // Row nodes and the labels inside them, parallel arrays keyed by build order.
    // Rows are positioned ABSOLUTELY rather than parented to a VBoxContainer: a
    // container owns its children's positions, which is exactly what a swap
    // animation needs to control.
    private readonly List<string> _rowIds = new();
    private readonly List<Control> _rows = new();
    private readonly List<Label> _rankLabels = new();
    private readonly List<Label> _scoreLabels = new();
    private readonly List<int> _shownScores = new();

    /// <summary>Current top-to-bottom ranking, as ids. Compared against a freshly
    /// computed one to decide whether anything actually needs to move.</summary>
    private readonly List<string> _order = new();
    private readonly List<string> _nextOrder = new();

    private ulong _nextReorderMsec;
    private Tween? _tween;

    private float _rowHeight;
    private float _padding;

#if DEV_TOOLS
    /// <summary>The live board, so a dev command can reach it. Dev-only: this adds
    /// no production surface, and being null outside a match is how "/lb" knows it
    /// has nothing to drive.</summary>
    public static LeaderboardView? Current { get; private set; }

    /// <summary>True while fake players are on the board instead of the match.</summary>
    public bool DemoActive { get; private set; }

    private static readonly string[] DemoNames =
    {
        "ricky", "bobby", "giffer", "mox", "pell", "juno", "vance", "wren",
    };

    // Faster than the reorder beat on purpose: swaps need to queue up behind each
    // other, or the glide only ever gets exercised once and looks fine by luck.
    private const ulong DemoScoreIntervalMsec = 1200;

    // A real GameState, not a parallel score model. The demo therefore drives the
    // SAME ApplyScores stamping and the SAME Reorder the match uses — so what you
    // are watching is the shipping tie-break, not a lookalike that could drift.
    private readonly GameState _demoState = new();
    private readonly Dictionary<string, int> _demoTally = new();
    private readonly RandomNumberGenerator _demoRng = new();
    private ulong _nextDemoScoreMsec;
#endif

    public override void _Ready()
    {
#if DEV_TOOLS
        Current = this;
        _demoRng.Randomize();
#endif
        float vpY = GetViewportSize().Y;
        _rowHeight = RowHeightHFrac * vpY;
        _padding = PaddingHFrac * vpY;

        _panel = new Panel { MouseFilter = Control.MouseFilterEnum.Ignore };
        _panel.SetAnchorsPreset(Control.LayoutPreset.TopLeft);
        _panel.Position = new Vector2(MarginHFrac * vpY, MarginHFrac * vpY);
        _panel.AddThemeStyleboxOverride("panel", PanelStyle());
        AddChild(_panel);

        BuildRows(Roster(null));
    }

    /// <summary>
    /// Repaint from the model. Scores are restated on every call so a point shows
    /// the instant the server confirms it; the ORDER is recomputed only once the
    /// reorder interval has elapsed, which is the whole reason the two are split.
    /// </summary>
    public void SyncFrom(GameState state)
    {
#if DEV_TOOLS
        // The demo owns the board while it is on; live scores would fight it.
        if (DemoActive)
            return;
#endif
        // A player holding points who was not in the seating roster at build time
        // (a late arrival, or a rejoin that rebuilt the roster) needs a row before
        // anything can be drawn for them. Scanned rather than rebuilt-and-compared:
        // this runs every frame, and Roster() allocates.
        foreach (var pid in state.Scores.Keys)
        {
            if (_rowIds.Contains(pid))
                continue;
            BuildRows(Roster(state));
            break;
        }

        Apply(state);
    }

    /// <summary>Restate the scores and, once the beat has come round, the order.
    /// Split out of <see cref="SyncFrom"/> so the dev demo drives the identical
    /// path rather than a copy of it.</summary>
    private void Apply(GameState state)
    {
        for (int i = 0; i < _rowIds.Count; i++)
        {
            int score = state.Scores.GetValueOrDefault(_rowIds[i], 0);
            if (_shownScores[i] == score)
                continue; // Label.Text invalidates layout; only pay for real changes.
            _shownScores[i] = score;
            _scoreLabels[i].Text = score.ToString();
        }

        ulong now = Time.GetTicksMsec();
        if (now < _nextReorderMsec)
            return;
        _nextReorderMsec = now + ReorderIntervalMsec;
        Reorder(state);
    }

    // ---- ranking ----

    /// <summary>
    /// Settle a new ranking and animate anything that moved.
    ///
    /// Order is score descending, then whoever REACHED that score first, then seat
    /// order. The middle key is the interesting one: the score frame carries the
    /// whole tally and never says who just scored, so <c>GameState.ApplyScores</c>
    /// stamps what moved and this reads those stamps. The stamps are local clock
    /// readings and mean nothing between machines — but every client receives the
    /// same broadcasts in the same order, so the ordering they produce agrees on
    /// every screen. Seat order is the final key because it is frozen at match
    /// start and identical everywhere, so a board where nobody has scored is
    /// stable rather than arbitrary.
    /// </summary>
    private void Reorder(GameState state)
    {
        _nextOrder.Clear();
        _nextOrder.AddRange(_rowIds);
        _nextOrder.Sort((a, b) =>
        {
            int sa = state.Scores.GetValueOrDefault(a, 0);
            int sb = state.Scores.GetValueOrDefault(b, 0);
            if (sa != sb)
                return sb.CompareTo(sa);

            ulong ta = state.ScoreReachedAtMsec.GetValueOrDefault(a, 0UL);
            ulong tb = state.ScoreReachedAtMsec.GetValueOrDefault(b, 0UL);
            if (ta != tb)
                return ta.CompareTo(tb);

            return SeatIndex(a).CompareTo(SeatIndex(b));
        });

        if (SameOrder(_nextOrder))
            return;

        _order.Clear();
        _order.AddRange(_nextOrder);

        // Rank numbers belong to the position, not the player, so they are restated
        // immediately while the rows are still travelling to meet them.
        for (int rank = 0; rank < _order.Count; rank++)
            _rankLabels[IndexOf(_order[rank])].Text = $"{rank + 1}.";

        // One tween for the whole board. Killing the in-flight one means a re-rank
        // landing mid-glide simply retargets from wherever each row currently is,
        // rather than stacking two animations that fight over the same position.
        if (_tween != null && _tween.IsValid())
            _tween.Kill();
        _tween = CreateTween()
            .SetParallel(true)
            .SetTrans(Tween.TransitionType.Sine)
            .SetEase(Tween.EaseType.InOut);

        for (int rank = 0; rank < _order.Count; rank++)
        {
            var row = _rows[IndexOf(_order[rank])];
            float targetY = _padding + rank * _rowHeight;
            if (!Mathf.IsEqualApprox(row.Position.Y, targetY))
                _tween.TweenProperty(row, "position:y", targetY, SwapSeconds);
        }
    }

    private int SeatIndex(string playerId)
    {
        var ctx = SessionContext.Instance;
        int i = ctx?.SeatOrder.IndexOf(playerId) ?? -1;
        // Anyone outside the frozen seating sorts after it, in a stable place.
        return i >= 0 ? i : int.MaxValue;
    }

    private int IndexOf(string playerId) => _rowIds.IndexOf(playerId);

    private bool SameOrder(List<string> candidate)
    {
        if (_order.Count != candidate.Count)
            return false;
        for (int i = 0; i < candidate.Count; i++)
            if (_order[i] != candidate[i])
                return false;
        return true;
    }

    // ---- roster and rows ----

    /// <summary>Everyone who should have a row: the frozen seating roster (falling
    /// back to the live lobby roster before a match freezes one), plus anyone
    /// holding points who is missing from it. Mirrors <c>EndGameMenu.Populate</c>,
    /// so the in-match board and the end screen can never disagree about who is
    /// in the match.</summary>
    private static List<string> Roster(GameState? state)
    {
        var ctx = SessionContext.Instance;
        var roster = new List<string>();
        if (ctx != null)
            roster.AddRange(ctx.SeatOrder.Count > 0 ? ctx.SeatOrder : ctx.PlayerIds);

        if (state != null)
            foreach (var pid in state.Scores.Keys)
                if (!roster.Contains(pid))
                    roster.Add(pid);

        return roster;
    }

    /// <summary><paramref name="nameOf"/> overrides how a row is labelled; the dev
    /// demo uses it because SessionContext would render a fake id as "Player ricky".
    /// A parameter rather than a field on purpose — this is not a reintroduction of
    /// the NameResolver that came off View2D.</summary>
    private void BuildRows(List<string> roster, Func<string, string>? nameOf = null)
    {
        // Kill first: the tween targets these row nodes, and freeing a node out
        // from under a running tween is an error, not a no-op.
        if (_tween != null && _tween.IsValid())
            _tween.Kill();
        _tween = null;

        // Rank these rows on the next sync rather than leaving them in build order
        // until the beat comes round.
        _nextReorderMsec = 0;

        foreach (var row in _rows)
            row.QueueFree();

        _rowIds.Clear();
        _rows.Clear();
        _rankLabels.Clear();
        _scoreLabels.Clear();
        _shownScores.Clear();
        _order.Clear();

        float vpY = GetViewportSize().Y;
        float width = PanelWidthHFrac * vpY;
        int fontSize = Mathf.RoundToInt(FontHFrac * vpY);

        _panel.Size = new Vector2(width, _padding * 2 + roster.Count * _rowHeight);

        float rowWidth = width - _padding * 2;
        float rankWidth = _rowHeight * 1.2f;
        float scoreWidth = _rowHeight * 2f;
        var ctx = SessionContext.Instance;

        for (int i = 0; i < roster.Count; i++)
        {
            string pid = roster[i];

            var row = new Control
            {
                MouseFilter = Control.MouseFilterEnum.Ignore,
                Position = new Vector2(_padding, _padding + i * _rowHeight),
                Size = new Vector2(rowWidth, _rowHeight),
            };
            _panel.AddChild(row);

            var rank = MakeLabel($"{i + 1}.", fontSize, NeonBlue, HorizontalAlignment.Left);
            rank.Position = Vector2.Zero;
            rank.Size = new Vector2(rankWidth, _rowHeight);
            row.AddChild(rank);

            string display = nameOf?.Invoke(pid) ?? ctx?.DisplayNameFor(pid) ?? pid;
            var name = MakeLabel(display, fontSize, NameColor, HorizontalAlignment.Left);
            name.Position = new Vector2(rankWidth, 0);
            name.Size = new Vector2(rowWidth - rankWidth - scoreWidth, _rowHeight);
            // A long username is clipped rather than allowed to shove the score off
            // the panel; the 20-char cap makes this a corner case, not the norm.
            name.ClipText = true;
            row.AddChild(name);

            var score = MakeLabel("0", fontSize, NeonBlue, HorizontalAlignment.Right);
            score.Position = new Vector2(rowWidth - scoreWidth, 0);
            score.Size = new Vector2(scoreWidth, _rowHeight);
            row.AddChild(score);

            _rowIds.Add(pid);
            _rows.Add(row);
            _rankLabels.Add(rank);
            _scoreLabels.Add(score);
            _shownScores.Add(0);
            _order.Add(pid);
        }
    }

    private static Label MakeLabel(string text, int fontSize, Color color, HorizontalAlignment align)
    {
        var label = new Label
        {
            Text = text,
            MouseFilter = Control.MouseFilterEnum.Ignore,
            HorizontalAlignment = align,
            VerticalAlignment = VerticalAlignment.Center,
        };
        label.AddThemeFontSizeOverride("font_size", fontSize);
        label.AddThemeColorOverride("font_color", color);
        return label;
    }

    /// <summary>Neon edge over a translucent tint of the same colour, with a soft
    /// glow so the border reads as lit rather than merely drawn. Border width and
    /// corner radius match the panel language used everywhere else in the UI.</summary>
    private static StyleBoxFlat PanelStyle()
    {
        var box = new StyleBoxFlat
        {
            BgColor = new Color(NeonBlue, FillAlpha),
            BorderColor = NeonBlue,
            CornerRadiusTopLeft = 8,
            CornerRadiusTopRight = 8,
            CornerRadiusBottomRight = 8,
            CornerRadiusBottomLeft = 8,
            ShadowColor = new Color(NeonBlue, 0.35f),
            ShadowSize = 12,
        };
        box.SetBorderWidthAll(2);
        return box;
    }

    private Vector2 GetViewportSize() => GetViewport().GetVisibleRect().Size;

#if DEV_TOOLS
    /// <summary>
    /// Fill the board with fake players and drive their scores, or hand it back to
    /// the match. Reached from <c>/lb</c>; see <c>DevCommands</c> for the gating.
    ///
    /// Switching off rebuilds from the real roster; the next <see cref="SyncFrom"/>
    /// (one frame later) restates the live scores, and BuildRows has already forced
    /// the ranking to settle immediately rather than at the next beat.
    /// </summary>
    public void SetDemo(bool on, int players)
    {
        DemoActive = on;
        _demoTally.Clear();

        if (!on)
        {
            BuildRows(Roster(null));
            return;
        }

        var ids = new List<string>();
        for (int i = 0; i < players && i < DemoNames.Length; i++)
        {
            ids.Add(DemoNames[i]);
            _demoTally[DemoNames[i]] = 0;
        }

        // The fake ids ARE the names, so the override is identity.
        BuildRows(ids, pid => pid);
        _demoState.ApplyScores(_demoTally);
        _nextDemoScoreMsec = Time.GetTicksMsec() + DemoScoreIntervalMsec;
    }

    public override void _Process(double delta)
    {
        if (!DemoActive || _rowIds.Count == 0)
            return;

        ulong now = Time.GetTicksMsec();
        if (now >= _nextDemoScoreMsec)
        {
            _nextDemoScoreMsec = now + DemoScoreIntervalMsec;

            // One random player gains 1 or 2 — the same values a real goal and a
            // split ball are worth, so the board moves the way a match moves it.
            string pid = _rowIds[(int)(_demoRng.Randi() % (uint)_rowIds.Count)];
            _demoTally[pid] = _demoTally.GetValueOrDefault(pid, 0) + (int)(_demoRng.Randi() % 2) + 1;
            _demoState.ApplyScores(_demoTally);
        }

        Apply(_demoState);
    }

    public override void _ExitTree()
    {
        if (Current == this)
            Current = null;
    }
#endif
}
