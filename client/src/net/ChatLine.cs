namespace BriskaBlast.Net;

/// <summary>
/// One chat line as broadcast by the server.
///
/// A record rather than more event arguments: chat has grown a sender kind and
/// will likely grow more, and four positional strings at a call site stop being
/// readable.
/// </summary>
/// <param name="From">Server-attested sender id. <em>Empty for a moderator
/// line</em> — a moderator speaks through the admin panel and has no player
/// account, so this must not be fed to a roster lookup.</param>
/// <param name="Username">The display name to render. For a moderator this is
/// either their real name or the generic <c>Mod</c>, depending on the anonymity
/// toggle they chose; the client is not told which, by design.</param>
/// <param name="Text">The message body. Blacklisted words arrive already masked
/// — the server censors before broadcast, so the raw word never reaches a
/// client and there is nothing to filter here.</param>
/// <param name="IsModerator">True when a moderator spoke into the session
/// rather than a player. Drives the distinct styling.</param>
/// <param name="BodyId">The server's moderation id for this line, and the only
/// identifier a client ever gets for a chat message. It exists so a later
/// <c>chat_body_deleted</c> can name <em>which</em> displayed line to remove;
/// nothing can be looked up with it. Empty when the server did not record the
/// line (a moderation outage never silences chat), in which case the line simply
/// cannot be targeted.</param>
public readonly record struct ChatLine(
    string From,
    string Username,
    string Text,
    bool IsModerator,
    string BodyId);
