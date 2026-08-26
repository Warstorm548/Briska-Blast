using System;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.UI.Chat;

/// <summary>
/// Reusable chat panel: a notice banner, an optional header, the log, and the
/// input. Used by the lobby at full size and by the match as a bottom-left
/// overlay, so a moderation change lands in both places at once.
///
/// A pure view over <see cref="ChatLog"/>. It never holds the transcript — that
/// belongs to <see cref="MatchFlow"/> so it can outlive every scene — and it
/// never touches the socket to receive. Sending goes through
/// <see cref="MatchFlow.SendChat"/>, and nothing is echoed locally: the server
/// broadcasts the line back to the sender too, which is what keeps every
/// client's transcript identical.
/// </summary>
public partial class ChatPanel : PanelContainer
{
    // Moderator chat lines are tinted so staff never read as another player.
    // The admin panel's own MOD badge uses the same blue (#58a6ff).
    private static readonly Color ModeratorChatColor = new("58a6ff");

    // A warning aimed at this player. Amber rather than the moderator blue: it is
    // not someone talking, it is an action taken against them.
    private static readonly Color WarningChatColor = new("d29922");

    // A chat ban aimed at this player. Red rather than the warning's amber — the
    // colour is the only thing that distinguishes a one-off notice from a
    // permanent loss of chat, and the two frames are otherwise identical.
    private static readonly Color BanChatColor = new("f85149");

    /// <summary>The input took or lost keyboard focus. The match uses this to
    /// suspend and restore player controls; the lobby ignores it.</summary>
    public event Action<bool>? InputFocusChanged;

    /// <summary>True while the input holds keyboard focus.</summary>
    public bool InputFocused { get; private set; }

    private ChatLog? _log;

    public override void _Ready()
    {
        var input = GetNode<LineEdit>("%ChatInput");

        // Godot 4.4 split a LineEdit's EDIT mode out of its focus state, and
        // keep_editing_on_text_submit defaults to false: Enter emits
        // text_submitted and then leaves edit mode while KEEPING focus. That is
        // the bug 0.33.0 aimed at and missed — it answered with GrabFocus on a
        // control that had never lost focus, so the call did nothing and the
        // caret stayed dead. Pressing Enter again was the only way back, because
        // Enter on a focused LineEdit re-enters edit mode: the workaround players
        // found is the diagnosis. Holding edit mode across a submit is precisely
        // what this property is for. The empty-Enter exit in OnSubmitted still
        // works, because it releases focus outright and editing ends with it.
        input.KeepEditingOnTextSubmit = true;

        input.TextSubmitted += OnSubmitted;
        input.FocusEntered += () => SetFocused(true);
        input.FocusExited += () => SetFocused(false);

        GetNode<Button>("%NoticeDismiss").Pressed += () =>
            GetNode<Control>("%NoticeRow").Visible = false;

        // Drop the scene's sample line: the log fills only from server-broadcast
        // frames, so every client shows the same transcript.
        GetNode<RichTextLabel>("%ChatLog").Clear();
    }

    public override void _ExitTree() => Unbind();

    /// <summary>Show or hide the "Chat" caption (the in-match overlay hides it —
    /// the strip is too short to spend a line naming itself).</summary>
    public void ShowHeader(bool visible) => GetNode<Label>("%ChatHeader").Visible = visible;

    /// <summary>
    /// Tighten the padding. The in-match panel rests inside a strip one action-bar
    /// slot tall, where the input alone claims about half the height — every pixel
    /// not spent on chrome is a line of history instead. The lobby keeps the
    /// roomier spacing, where there is no such pressure.
    /// </summary>
    public void SetCompact(bool compact)
    {
        var margins = GetNode<MarginContainer>("Margins");
        margins.AddThemeConstantOverride("margin_top", compact ? 6 : 12);
        margins.AddThemeConstantOverride("margin_bottom", compact ? 6 : 12);
        margins.AddThemeConstantOverride("margin_left", compact ? 10 : 16);
        margins.AddThemeConstantOverride("margin_right", compact ? 10 : 16);
        GetNode<VBoxContainer>("Margins/Content")
            .AddThemeConstantOverride("separation", compact ? 4 : 8);
    }

    /// <summary>
    /// Stop the panel responding to the mouse at all, so a click falls through to
    /// whatever is behind it instead of focusing the input.
    ///
    /// The match hides the cursor, and a blind click that focused chat would
    /// suspend the player's controls with nothing on screen to say why. Keyboard
    /// focus is untouched — <c>mouse_filter</c> gates click routing, not
    /// <c>GrabFocus</c>, so T and / still open the input. Setting
    /// <c>focus_mode</c> instead would have blocked both.
    ///
    /// One-way, and only the match calls it: the defaults being undone here
    /// differ per node type (a Label ignores the mouse already, a Button does
    /// not), so there is no single value to hand back. The lobby simply never
    /// calls it. Same reasoning as the action bar, which is Ignore throughout for
    /// being keyboard-driven — see <c>HotbarView</c>.
    /// </summary>
    public void MakeClickThrough()
    {
        MouseFilter = MouseFilterEnum.Ignore;
        // owned: false — the children belong to whichever scene instanced this
        // panel (the lobby's tree, or InGameChat at runtime), not to the panel.
        foreach (var child in FindChildren("*", "Control", recursive: true, owned: false))
            ((Control)child).MouseFilter = MouseFilterEnum.Ignore;
    }

    /// <summary>Show or hide the notice banner's ✕. The match hides it: with no
    /// cursor there is nothing to click it with, and a moderator notice there is
    /// meant to stay until a moderator replaces it.</summary>
    public void ShowDismissButton(bool visible) =>
        GetNode<Button>("%NoticeDismiss").Visible = visible;

    /// <summary>
    /// Render <paramref name="log"/> and follow it from here on.
    ///
    /// Draws the existing entries BEFORE subscribing, the same pull-then-subscribe
    /// ordering <c>PreparingScreen</c> uses: entries added between the scene change
    /// and this call would otherwise be lost.
    /// </summary>
    public void Bind(ChatLog log)
    {
        Unbind();
        _log = log;
        // Backlog only — deliberately not through OnEntryAdded, or entering the
        // match would re-raise a banner for every warning already dealt with in
        // the lobby.
        Redraw();
        log.EntryAdded += OnEntryAdded;
        log.Redrawn += Redraw;
    }

    private void Unbind()
    {
        if (_log == null)
            return;
        _log.EntryAdded -= OnEntryAdded;
        _log.Redrawn -= Redraw;
        _log = null;
    }

    /// <summary>Give the input keyboard focus, optionally seeded with
    /// <paramref name="prefill"/> (the "/" that command-style opening leaves in
    /// place for a future command parser).</summary>
    public void FocusInput(string prefill = "")
    {
        var input = GetNode<LineEdit>("%ChatInput");
        input.Text = prefill;
        input.CaretColumn = prefill.Length;
        input.GrabFocus();
        // GrabFocus only starts edit mode as a side effect of the focus CHANGING,
        // so it does nothing on an input already focused but not editing — the
        // state anything that ends editing without taking focus leaves behind
        // (Escape, now that a submit no longer does). Edit() is the direct way in
        // and no-ops when already editing, so T and / open the caret from either
        // state rather than only from a cold start.
        input.Edit();
    }

    /// <summary>Drop keyboard focus, handing control back to whatever owns it.</summary>
    public void ReleaseInput() => GetNode<LineEdit>("%ChatInput").ReleaseFocus();

    private void SetFocused(bool focused)
    {
        if (InputFocused == focused)
            return;
        InputFocused = focused;
        InputFocusChanged?.Invoke(focused);
    }

    // Enter in the input. Trim, send through the orchestrator, then clear the
    // field — focus AND edit mode are deliberately KEPT, because sending is not
    // leaving. Both survive on their own now; see KeepEditingOnTextSubmit in
    // _Ready for why keeping the caret is a property and not a re-grab.
    private void OnSubmitted(string text)
    {
        var input = GetNode<LineEdit>("%ChatInput");
        var trimmed = text.Trim();

        // Enter on an empty box is the way out, and a bare "/" counts as empty:
        // opening in command style puts it there, so backing out of a command
        // must not post a lone slash to the session.
        if (trimmed.Length == 0 || trimmed == "/")
        {
            input.Clear();
            ReleaseInput();
            return;
        }

#if DEV_TOOLS
        // Dev builds only, and only in the editor — see DevCommands for both gates.
        // An unrecognised "/…" falls through and posts as chat, exactly as 0.32.0
        // documented, so the contract is not quietly different here.
        if (Dev.DevCommands.TryHandle(trimmed, MatchFlow.Instance.Chat))
        {
            input.Clear();
            return;
        }
#endif

        MatchFlow.Instance.SendChat(trimmed);
        // Clear only. Nothing has to be restored: the input holds on to both focus
        // and edit mode across the submit, so the caret is already where the player
        // left it, ready for the next line.
        input.Clear();
    }

    // A line was appended. Drawing just this one keeps the common case cheap and
    // leaves RichTextLabel's scroll_following to do its job.
    private void OnEntryAdded(ChatEntry entry)
    {
        DrawEntry(GetNode<RichTextLabel>("%ChatLog"), entry);

        // A notice aimed at this player is shown twice over: in the log where the
        // conversation is, and on a banner that cannot scroll away. The server
        // never resends a warning, so the log alone would let it slip past.
        if (entry.IsBan)
            ShowNotice($"⛔ Chat banned by a moderator: {entry.Text}", BanChatColor);
        else if (entry.IsWarning)
            ShowNotice($"⚠ Warning from a moderator: {entry.Text}", WarningChatColor);
    }

    // Rebuild from the transcript. Only a deletion or the handoff trim needs
    // this — every other change is an append.
    private void Redraw()
    {
        var log = GetNode<RichTextLabel>("%ChatLog");
        log.Clear();
        if (_log == null)
            return;
        foreach (var entry in _log.Entries)
            DrawEntry(log, entry);
    }

    // Draw one entry. Every server- and user-supplied string goes through AddText
    // (never AppendText), and colour/bold come from the Push*/Pop API rather than
    // interpolated BBCode — the label has bbcode_enabled, so building tags into a
    // string would be a tag-injection hole.
    private static void DrawEntry(RichTextLabel log, ChatEntry entry)
    {
        // A deleted line leaves no trace in the log: the placeholder exists to
        // hold its position in the list, not to show a gap on screen.
        if (entry.Deleted)
            return;

        // Both notice types read identically apart from their tag and colour, so
        // they share one branch rather than two near-copies.
        if (entry.IsWarning || entry.IsBan)
        {
            log.PushColor(entry.IsBan ? BanChatColor : WarningChatColor);
            log.PushBold();
            log.AddText(entry.IsBan ? "[CHAT BANNED]" : "[WARNING]");
            log.Pop();
            log.AddText($" {entry.Text}\n");
            log.Pop();
            return;
        }

        if (entry.IsModerator)
        {
            // The name is whatever the moderator chose to appear as; the client
            // is deliberately not told who is behind an anonymous "Mod".
            log.PushColor(ModeratorChatColor);
            log.PushBold();
            log.AddText($"[MOD] {entry.Name}");
            log.Pop();
            log.AddText($": {entry.Text}\n");
            log.Pop();
            return;
        }

        log.PushBold();
        log.AddText(entry.Name);
        log.Pop();
        log.AddText($": {entry.Text}\n");
    }

    // Replace rather than stack: the newest notice is the current one, and a
    // growing pile of banners would push the chat itself off screen. The colour
    // is re-applied every time so a ban landing after a warning recolours the
    // banner it inherits.
    private void ShowNotice(string message, Color colour)
    {
        var label = GetNode<Label>("%NoticeLabel");
        label.AddThemeColorOverride("font_color", colour);
        label.Text = message;
        GetNode<Control>("%NoticeRow").Visible = true;
    }
}
