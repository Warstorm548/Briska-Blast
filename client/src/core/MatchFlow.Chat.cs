using BriskaBlast.Net;

namespace BriskaBlast.Core;

/// <summary>The session's chat transcript. Owned by the orchestrator rather than
/// by any view so it spans the lobby, Preparing and the match — including the
/// stretch where no view is mounted at all.</summary>
public partial class MatchFlow
{
    /// <summary>The session's chat transcript. Owned here rather than by the view
    /// that draws it so it survives every scene change: the lobby, the Preparing
    /// screen and the match all bind the same log, and lines that arrive while no
    /// view is mounted are still recorded. Cleared on teardown, never rebuilt from
    /// the server.</summary>
    public ChatLog Chat { get; } = new();

    /// <summary>Send a chat line through the live signaling socket, so a chat
    /// view never touches the socket to send. Nothing is rendered locally — the
    /// server echoes the line back to the sender too, and rendering on receipt is
    /// what keeps every client's transcript identical.</summary>
    public void SendChat(string text) => Signaling?.SendChatMessage(text);

    // ---- chat transcript (recorded here so it outlives every view) ----

    // A chat line broadcast by the server (ours or a peer's). Keep the roster's
    // name map in step with what chat reports, then label via the same
    // DisplayNameFor fallback (Player <id>) the rest of the client uses.
    //
    // A moderator line is the exception: it carries no sender id, so it must not
    // go through the roster at all — feeding it an empty id would register a
    // phantom roster entry and then label the line from it.
    private void OnChatMessage(ChatLine line)
    {
        if (line.IsModerator)
        {
            Chat.Add(new ChatEntry
            {
                BodyId = line.BodyId,
                Name = line.Username,
                Text = line.Text,
                IsModerator = true,
            });
            return;
        }

        var ctx = SessionContext.Instance;
        if (!string.IsNullOrEmpty(line.Username))
            ctx.SetUsername(line.From, line.Username);
        Chat.Add(new ChatEntry
        {
            BodyId = line.BodyId,
            Name = ctx.DisplayNameFor(line.From),
            Text = line.Text,
        });
    }

    // A moderator warned this player. Only this client receives it, and it is
    // never queued. Recorded as an entry; the bound view is what also raises the
    // banner, since a line that only appeared in the log would scroll away.
    private void OnChatWarning(string reason) =>
        Chat.Add(new ChatEntry { Text = reason, IsWarning = true });

    // This player's chat privileges were revoked. Same shape as a warning but
    // permanent, so the view renders it red.
    //
    // Arrives when the ban lands and again on every send the server refuses, so a
    // player who was offline at the time still finds out the moment they try to
    // speak. The repeat is the point — it is the answer to what they just did.
    private void OnChatBanned(string reason) =>
        Chat.Add(new ChatEntry { Text = reason, IsBan = true });

    // A moderator withdrew a message from everyone in the session.
    private void OnChatBodyDeleted(string bodyId) => Chat.MarkDeleted(bodyId);

    /// <summary>
    /// The chat handoff, run as the flow enters Preparing from any of its three
    /// convergent paths: the lobby conversation becomes the match's conversation.
    ///
    /// Nothing is copied — the transcript already lives on this orchestrator,
    /// which is what lets it keep recording through Preparing while no view is
    /// mounted. Handing a snapshot over at this boundary instead would drop
    /// everything broadcast during the phase, which is the bug this replaces.
    /// What happens here is the bound: the match starts from a capped window.
    /// Logged so the handoff is visible in the per-run client log next to the
    /// mesh and ready-barrier lines.
    /// </summary>
    private void CarryChatIntoMatch()
    {
        var (kept, total) = Chat.CarryIntoMatch();
        Log.Info("match.flow", $"chat carried into match: kept {kept} of {total} lines.");
    }
}
