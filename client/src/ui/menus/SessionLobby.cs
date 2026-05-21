using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Menus;

public partial class SessionLobby : Control
{
    private HBoxContainer[] _slots = null!;

    public override void _Ready()
    {
        _slots = new[]
        {
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot1"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot2"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot3"),
            GetNode<HBoxContainer>("RightPanel/RightMargins/RightContent/PlayerSlots/Slot4"),
        };

        for (int i = 0; i < _slots.Length; i++)
        {
            var idx = i;
            _slots[i].GetNode<Button>("Promote").Pressed += () => OnPromote(idx);
        }

        GetNode<Button>("%StartSessionButton").Pressed += OnStartPressed;
        GetNode<Button>("%CancelSessionButton").Pressed += OnCancelOrLeavePressed;
        GetNode<Button>("%ReturnToSetupButton").Pressed += OnReturnToSetupPressed;

        RefreshFromContext();

#if DEBUG
        GD.Print("[lobby debug] F1 = toggle host view, F2 = add player, F3 = remove last player");
#endif
    }

#if DEBUG
    public override void _UnhandledInput(InputEvent @event)
    {
        if (@event is not InputEventKey keyEvent || !keyEvent.Pressed || keyEvent.Echo) return;
        var ctx = SessionContext.Instance;

        switch (keyEvent.Keycode)
        {
            case Key.F1:
                ctx.LocalPlayerIsHost = !ctx.LocalPlayerIsHost;
                GD.Print($"[lobby debug] LocalPlayerIsHost = {ctx.LocalPlayerIsHost}");
                RefreshFromContext();
                break;
            case Key.F2:
                if (ctx.PlayerNames.Count < ctx.MaxPlayers)
                {
                    var n = ctx.PlayerNames.Count + 1;
                    ctx.PlayerNames.Add($"Player Username {n}");
                    GD.Print($"[lobby debug] added Player Username {n}");
                    RefreshFromContext();
                }
                break;
            case Key.F3:
                if (ctx.PlayerNames.Count > 1)
                {
                    ctx.PlayerNames.RemoveAt(ctx.PlayerNames.Count - 1);
                    if (ctx.HostIndex >= ctx.PlayerNames.Count) ctx.HostIndex = 0;
                    GD.Print($"[lobby debug] removed last player; remaining = {ctx.PlayerNames.Count}");
                    RefreshFromContext();
                }
                break;
        }
    }
#endif

    private void RefreshFromContext()
    {
        var ctx = SessionContext.Instance;

        GetNode<Label>("%SessionCodeValue").Text =
            string.IsNullOrEmpty(ctx.SessionCode) ? "------" : ctx.SessionCode;
        GetNode<Label>("%ModeValue").Text =
            string.IsNullOrEmpty(ctx.GameMode) ? "—" : ctx.GameMode;
        GetNode<Label>("%MaxPlayersValue").Text = ctx.MaxPlayers.ToString();

        for (int i = 0; i < _slots.Length; i++)
        {
            var slot = _slots[i];
            var nameLabel = slot.GetNode<Label>("Name");
            var isHostLabel = slot.GetNode<Label>("IsHost");
            var promoteBtn = slot.GetNode<Button>("Promote");

            bool slotUsed = i < ctx.PlayerNames.Count;
            bool slotIsHost = slotUsed && i == ctx.HostIndex;

            nameLabel.Text = slotUsed ? ctx.PlayerNames[i] : "Empty Slot";
            isHostLabel.Visible = slotIsHost;
            promoteBtn.Visible = slotUsed && !slotIsHost && ctx.LocalPlayerIsHost;
        }

        GetNode<Button>("%StartSessionButton").Visible = ctx.LocalPlayerIsHost;
        GetNode<Button>("%ReturnToSetupButton").Visible = ctx.LocalPlayerIsHost;
        var cancelBtn = GetNode<Button>("%CancelSessionButton");
        cancelBtn.Visible = true;
        cancelBtn.Text = ctx.LocalPlayerIsHost ? "Cancel Session" : "Leave Session";
    }

    private void OnPromote(int slotIndex)
    {
        var ctx = SessionContext.Instance;
        ctx.PromoteToHost(slotIndex);
        ctx.LocalPlayerIsHost = false;
        RefreshFromContext();
    }

    private void OnStartPressed()
    {
        GD.Print($"Start: would start session {SessionContext.Instance.SessionCode}");
    }

    private void OnCancelOrLeavePressed()
    {
        SessionContext.Instance.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/MainMenu.tscn");
    }

    private void OnReturnToSetupPressed()
    {
        SessionContext.Instance.ClearSession();
        GetTree().ChangeSceneToFile("res://src/ui/menus/HostSetupMenu.tscn");
    }
}
