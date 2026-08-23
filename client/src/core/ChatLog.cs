using System;
using System.Collections.Generic;

namespace BriskaBlast.Core;

/// <summary>
/// One line this client received, kept so the log can be rebuilt.
///
/// The chat log is a single <c>RichTextLabel</c> buffer with no per-line nodes, so
/// there is nothing in the scene tree to address when a moderator deletes a
/// message. The owning <see cref="ChatLog"/>'s list is that address book.
/// </summary>
public sealed class ChatEntry
{
    public string BodyId = "";
    public string Name = "";
    public string Text = "";
    public bool IsModerator;
    public bool IsWarning;

    /// <summary>A chat ban. Rendered like a warning but red; kept as its own flag
    /// rather than a severity on <see cref="IsWarning"/> so a future third notice
    /// type does not have to reinterpret either.</summary>
    public bool IsBan;

    /// <summary>Deleted by a moderator: the text is wiped and rendering skips it,
    /// but the entry stays in place. Holding the position means a future restore
    /// could refill it where it belongs without the server having to describe
    /// where that is.</summary>
    public bool Deleted;
}

/// <summary>
/// The client's chat transcript for one session, owned by <see cref="MatchFlow"/>
/// and therefore alive from the moment the signaling socket opens until the
/// session tears down — across the lobby, the Preparing screen and the match.
///
/// It lives here rather than on the view that draws it for two reasons. A scene-
/// owned transcript died with the lobby, so a player entered a match with no
/// history; and nothing was subscribed to <c>chat_message</c> during Preparing, so
/// lines broadcast between Start and the mesh coming up were dropped outright.
/// Both are the same bug — the transcript outliving no scene — and both are fixed
/// by a single owner that outlives all of them.
///
/// Carryover is strictly local: this is what the client already heard. The server
/// is never asked to replay a transcript, so a client that joins late (or rejoins
/// after a process death) legitimately starts empty.
/// </summary>
public sealed class ChatLog
{
    /// <summary>How many lines survive the handoff into a match. A lobby can sit
    /// open for a long time, and the whole list is replayed on every moderator
    /// delete, so the match starts from a bounded window rather than everything
    /// ever said.</summary>
    public const int CarryLimit = 100;

    private readonly List<ChatEntry> _entries = new();

    /// <summary>Ordered oldest-first, mirroring what a bound view displays.</summary>
    public IReadOnlyList<ChatEntry> Entries => _entries;

    /// <summary>A line was appended. Views draw just this entry — the common case
    /// stays cheap and RichTextLabel's scroll_following keeps doing its job.</summary>
    public event Action<ChatEntry>? EntryAdded;

    /// <summary>The list changed in a way an append cannot express (a deletion, or
    /// the handoff trim). Views must clear and replay <see cref="Entries"/>.</summary>
    public event Action? Redrawn;

    public void Add(ChatEntry entry)
    {
        _entries.Add(entry);
        EntryAdded?.Invoke(entry);
    }

    /// <summary>
    /// A moderator withdrew a message from everyone in the session.
    ///
    /// The entry is emptied, not removed: rendering skips it so the visible log
    /// closes up with no gap, while the body id keeps its place in the order. A
    /// later restore could then refill it exactly where it was without the server
    /// needing to say where that is.
    /// </summary>
    public void MarkDeleted(string bodyId)
    {
        if (string.IsNullOrEmpty(bodyId))
            return;
        foreach (var entry in _entries)
        {
            // An unrecorded line carries no id, so an empty id must never match.
            if (entry.BodyId != bodyId)
                continue;
            entry.Text = "";
            entry.Name = "";
            entry.Deleted = true;
            Redrawn?.Invoke();
            return;
        }
        // Not found: this client joined after the message, never saw it, or it
        // fell outside the window kept by CarryIntoMatch.
    }

    /// <summary>
    /// The lifecycle handoff, run as the flow enters <c>Preparing</c>: the lobby
    /// conversation becomes the match's conversation.
    ///
    /// The transcript is not copied anywhere — it already lives here, which is
    /// what lets it keep recording through Preparing while no view is mounted.
    /// What this does is bound it: everything past <see cref="CarryLimit"/> is
    /// dropped so a long lobby session neither drags an unbounded list into the
    /// match nor makes the first in-game redraw proportional to it.
    /// </summary>
    /// <returns>(kept, total) as they were before the trim, for the caller to log.</returns>
    public (int Kept, int Total) CarryIntoMatch()
    {
        var total = _entries.Count;
        if (total <= CarryLimit)
            return (total, total);

        _entries.RemoveRange(0, total - CarryLimit);
        // A trim is not expressible as an append, and a bound view may already be
        // showing the lines that just went away.
        Redrawn?.Invoke();
        return (CarryLimit, total);
    }

    public void Clear()
    {
        _entries.Clear();
        Redrawn?.Invoke();
    }
}
