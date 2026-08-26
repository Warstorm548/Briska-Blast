#if DEV_TOOLS
using System;
using BriskaBlast.Core;
using BriskaBlast.UI;
using Godot;

namespace BriskaBlast.Dev;

/// <summary>
/// The dev-tools "/" command parser.
///
/// Game 0.32.0 added the <c>chat_command</c> key, which opens chat with the slash
/// already typed "so a future dev-tools parser has its prefix". This is that
/// parser, and it is the only thing that has ever read the prefix.
///
/// <para><b>Double-gated, on purpose.</b> The whole file is inside
/// <c>#if DEV_TOOLS</c>, which <c>BriskaBlast.csproj</c> defines only when the
/// release channel is <c>dev</c> — an ea or stable assembly does not contain this
/// code at all. On top of that, <see cref="TryHandle"/> refuses unless the process
/// is the Godot editor, so even a dev-channel export on a tester's machine carries
/// it inert. Neither gate is redundant: the first controls what ships, the second
/// controls what runs.</para>
///
/// <para>It lives in <c>src/dev/</c> rather than <c>src/core/</c> because it has to
/// reach a UI component, and Core does not reference UI anywhere else — that layer
/// is worth keeping clean for a tool that never ships.</para>
///
/// <para>Replies go straight into the local <see cref="ChatLog"/> and never touch
/// the socket, so no peer sees them and they render through the transcript's
/// existing path with no new drawing code.</para>
/// </summary>
public static class DevCommands
{
    /// <summary>Sender name on a command reply. Lowercase so it reads as machinery
    /// rather than as a player called "Dev".</summary>
    private const string ReplyName = "dev";

    private const int DefaultPlayers = 4;
    private const int MinPlayers = 2;
    private const int MaxPlayers = 8;

    /// <summary>
    /// Handle <paramref name="line"/> if it is a dev command.
    /// </summary>
    /// <returns>
    /// True when the line was consumed and the caller must NOT send it. False to
    /// let it post as ordinary chat — which is what an unrecognised "/…" line
    /// does, exactly as 0.32.0 documented. A typo posting to the session is
    /// better than a typo vanishing into a parser that silently ate it.
    /// </returns>
    public static bool TryHandle(string line, ChatLog log)
    {
        if (!line.StartsWith('/'))
            return false;

        // The runtime half of the gate. Checked before anything is parsed, so an
        // exported dev build treats every command as ordinary chat.
        if (!OS.HasFeature("editor"))
            return false;

        var parts = line[1..].Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length == 0)
            return false; // A bare "/" is chat's way out, handled before us.

        switch (parts[0].ToLowerInvariant())
        {
            case "help":
                Reply(log, $"commands: /help · /lb [{MinPlayers}-{MaxPlayers}] — toggle the leaderboard demo");
                return true;

            case "lb":
                return Leaderboard(log, parts);

            default:
                return false;
        }
    }

    /// <summary>
    /// Toggle the leaderboard demo, optionally with a player count.
    ///
    /// The board only exists inside a match, and the chat panel is shared with the
    /// lobby — so out there this answers rather than doing nothing, which would be
    /// indistinguishable from the command being broken.
    /// </summary>
    private static bool Leaderboard(ChatLog log, string[] parts)
    {
        var board = LeaderboardView.Current;
        if (board == null)
        {
            Reply(log, "/lb needs a match — there is no leaderboard here.");
            return true;
        }

        int players = DefaultPlayers;
        if (parts.Length > 1)
        {
            if (!int.TryParse(parts[1], out players))
            {
                Reply(log, $"/lb: '{parts[1]}' is not a number.");
                return true;
            }
            players = Mathf.Clamp(players, MinPlayers, MaxPlayers);
        }

        bool on = !board.DemoActive;
        board.SetDemo(on, players);
        Reply(log, on
            ? $"leaderboard demo ON — {players} fake players, live scores ignored until you /lb again"
            : "leaderboard demo OFF — real roster and scores restored");
        return true;
    }

    private static void Reply(ChatLog log, string text) =>
        log.Add(new ChatEntry { Name = ReplyName, Text = text });
}
#endif
