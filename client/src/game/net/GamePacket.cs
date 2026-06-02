using System;
using System.IO;
using System.Text;

namespace BriskaBlast.Game.Net;

/// <summary>Discriminator byte at the head of every game packet.</summary>
public enum GameMsg : byte
{
    BallHandoff = 1,
}

/// <summary>
/// A ball leaving one screen for the peer across a portal edge. The fields are
/// the <c>BallTransform</c> canonical form so this packet carries no notion of
/// "which physical edge" — the receiver maps it onto its own entry edge for
/// the sending peer.
/// </summary>
public readonly struct BallHandoffPacket
{
    public readonly int BallId;
    public readonly string LastHitterId;
    /// <summary>Outward-perpendicular speed at the exit edge (≥ 0), as a fraction
    /// of the sender's arena HEIGHT per second. The receiver multiplies by its own
    /// arena height, so entry pace/angle is independent of either screen's size.</summary>
    public readonly float Perp;
    /// <summary>Tangential speed along the exit edge (signed), as a fraction of the
    /// sender's arena HEIGHT per second (see <see cref="Perp"/>).</summary>
    public readonly float Tang;
    /// <summary>Position along the exit edge in [0, 1].</summary>
    public readonly float Along;
    /// <summary>Server-synced time (ms) when the sender emitted this — see
    /// <c>ServerClock</c>. The receiver fast-forwards the entering ball by
    /// <c>now − this</c> in that same shared frame, so the gap reflects real
    /// network transit rather than the offset between two machines' wall
    /// clocks.</summary>
    public readonly long SentTimestampMs;

    public BallHandoffPacket(int ballId, string lastHitterId,
        float perp, float tang, float along, long sentTimestampMs)
    {
        BallId = ballId;
        LastHitterId = lastHitterId;
        Perp = perp;
        Tang = tang;
        Along = along;
        SentTimestampMs = sentTimestampMs;
    }
}

/// <summary>
/// Compact binary framing for the game packets clients exchange directly over
/// <c>IPeerTransport</c> DataChannels. Both ends are C# (so BinaryWriter's
/// little-endian default is fine); a leading byte selects the variant.
/// JSON is too verbose for per-tick traffic — see
/// docs/architecture/game-architecture-summary.md §"Packet Contents".
/// </summary>
public static class GamePacket
{
    public static byte[] WriteBallHandoff(BallHandoffPacket p)
    {
        using var ms = new MemoryStream(48);
        using var w = new BinaryWriter(ms, Encoding.UTF8, leaveOpen: false);
        w.Write((byte)GameMsg.BallHandoff);
        w.Write(p.BallId);
        WriteShortString(w, p.LastHitterId);
        w.Write(p.Perp);
        w.Write(p.Tang);
        w.Write(p.Along);
        w.Write(p.SentTimestampMs);
        return ms.ToArray();
    }

    /// <summary>Returns true and fills <paramref name="pkt"/> when <paramref name="data"/>
    /// is a well-formed BallHandoff frame; false otherwise (unknown discriminator
    /// or truncated payload). Never throws on malformed input — peers can't
    /// crash us with a bad packet.</summary>
    public static bool TryReadBallHandoff(byte[] data, out BallHandoffPacket pkt)
    {
        pkt = default;
        if (data is null || data.Length < 2 || data[0] != (byte)GameMsg.BallHandoff)
            return false;

        try
        {
            using var ms = new MemoryStream(data, writable: false);
            using var r = new BinaryReader(ms, Encoding.UTF8, leaveOpen: false);
            r.ReadByte(); // discriminator
            int id = r.ReadInt32();
            string hitter = ReadShortString(r);
            float perp = r.ReadSingle();
            float tang = r.ReadSingle();
            float along = r.ReadSingle();
            long ts = r.ReadInt64();
            pkt = new BallHandoffPacket(id, hitter, perp, tang, along, ts);
            return true;
        }
        catch
        {
            return false;
        }
    }

    // Length-prefixed UTF-8, byte length. Player IDs are short — well under 255.
    private static void WriteShortString(BinaryWriter w, string s)
    {
        var bytes = Encoding.UTF8.GetBytes(s ?? string.Empty);
        if (bytes.Length > 255)
            throw new ArgumentException("short string exceeds 255 bytes", nameof(s));
        w.Write((byte)bytes.Length);
        w.Write(bytes);
    }

    private static string ReadShortString(BinaryReader r)
    {
        byte len = r.ReadByte();
        return Encoding.UTF8.GetString(r.ReadBytes(len));
    }
}
