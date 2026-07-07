using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

/// <summary>
/// Full-screen view for <see cref="MatchFlowState.Preparing"/>: shown between
/// the lobby (or a rejoin) and the game while the WebRTC mesh comes up. A thin
/// view over <see cref="MatchFlow"/> — it renders progress and forwards Cancel;
/// the orchestrator owns the timeout, the failure path, and the transition into
/// the game scene. No signaling subscriptions live here.
/// </summary>
public partial class PreparingScreen : Control
{
    private PreparingPanel _panel = null!;

    public override void _Ready()
    {
        _panel = GetNode<PreparingPanel>("%Panel");
        _panel.ShowCancel(true);
        _panel.CancelPressed += OnCancel;
        MatchFlow.Instance.PreparingProgress += OnProgress;
    }

    public override void _ExitTree()
    {
        _panel.CancelPressed -= OnCancel;
        MatchFlow.Instance.PreparingProgress -= OnProgress;
    }

    private void OnProgress(string status) => _panel.SetStatus(status);

    // Cancel = abandon the connection attempt. A plain (non-leave) close, so a
    // mid-match rejoin that's cancelled keeps its slot for the grace window.
    private void OnCancel() => MatchFlow.Instance.LeaveSession(sendLeaveFrame: false);
}
