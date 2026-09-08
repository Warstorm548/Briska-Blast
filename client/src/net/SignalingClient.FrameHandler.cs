using System;
using System.Text.Json;
using Godot;
using BriskaBlast.Core;

namespace BriskaBlast.Net;

/// <summary>The inbound half of the protocol: parse one server frame and raise the
/// matching event. Malformed or unknown frames are logged and dropped, never
/// thrown — one bad frame must not take the connection down.</summary>
public partial class SignalingClient
{
    /// <summary>Parse one server frame and dispatch it to the matching event
    /// (<c>identified</c>, <c>peer_joined</c>, host/peer reconnect, score updates,
    /// SDP/ICE relays, chat, …). Malformed or untyped frames are ignored.</summary>
    private void HandleFrame(string text)
    {
        try
        {
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            if (!root.TryGetProperty("type", out var typeEl))
                return;

            switch (typeEl.GetString())
            {
                case "identified":
                    Identified?.Invoke(
                        Str(root, "host_player_id"),
                        ReadStrings(root, "peers"),
                        ReadStrings(root, "seat_order"),
                        root.GetProperty("is_host").GetBoolean(),
                        ReadStringMap(root, "usernames"),
                        ReadIceServers(root));
                    break;
                case "peer_joined":
                    PeerJoined?.Invoke(Str(root, "player_id"), Str(root, "username"));
                    break;
                case "peer_left":
                    PeerLeft?.Invoke(Str(root, "player_id"), Str(root, "reason"));
                    break;
                case "host_changed":
                    HostChanged?.Invoke(Str(root, "player_id"));
                    break;
                case "host_reconnecting":
                    HostReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "host_reconnected":
                    HostReconnected?.Invoke(Str(root, "player_id"));
                    break;
                case "peer_reconnecting":
                    PeerReconnecting?.Invoke(Str(root, "player_id"), IntProp(root, "grace_secs"));
                    break;
                case "start_signaling":
                    StartSignaling?.Invoke(
                        Str(root, "gamemode"),
                        ReadWinCondition(root, "win_condition"),
                        ReadSpawnSettings(root, "spawn_settings"),
                        ReadLootSettings(root, "loot_settings"),
                        root.GetProperty("player_count").GetInt32(),
                        ReadStrings(root, "peers"),
                        ReadIceServers(root));
                    break;
                case "session_ended":
                    SessionEnded?.Invoke(Str(root, "reason"));
                    break;
                case "kicked":
                    Kicked?.Invoke(Str(root, "reason"));
                    break;
                case "score_update":
                    ScoreUpdate?.Invoke(ReadIntMap(root, "scores"));
                    break;
                case "game_over":
                    GameOver?.Invoke(Str(root, "winner_player_id"), ReadIntMap(root, "scores"));
                    break;
                case "chat_message":
                    // `kind` is absent on servers predating moderator chat; Str
                    // returns "" there, which reads as an ordinary player line.
                    // `body_id` is absent on servers predating deletion, and
                    // empty when the server could not record the line — either
                    // way the line renders and simply cannot be deleted.
                    ChatMessage?.Invoke(new ChatLine(
                        Str(root, "from"),
                        Str(root, "username"),
                        Str(root, "text"),
                        Str(root, "kind") == "moderator",
                        Str(root, "body_id")));
                    break;
                case "chat_warning":
                    ChatWarning?.Invoke(Str(root, "reason"));
                    break;
                case "chat_banned":
                    ChatBanned?.Invoke(Str(root, "reason"));
                    break;
                case "chat_body_deleted":
                    ChatBodyDeleted?.Invoke(Str(root, "body_id"));
                    break;
                case "match_started":
                    MatchStarted?.Invoke();
                    break;
                case "match_paused":
                    MatchPaused?.Invoke(
                        Str(root, "player_id"),
                        Str(root, "username"),
                        IntProp(root, "resume_timeout_secs"));
                    break;
                case "match_resumed":
                    MatchResumed?.Invoke(IntProp(root, "countdown_secs"));
                    break;
                case "time_sync":
                {
                    // T4 = now; fold this round-trip into the server-clock offset.
                    long t1 = LongProp(root, "client_send_ms");
                    long t4 = (long)Time.GetTicksMsec();
                    long serverMs = LongProp(root, "server_ms");
                    // TEMP diagnostic (fix/ball-handoff-entry-position): the entry
                    // fast-forward trusts this offset, so a biased one flings
                    // incoming balls inward. Log the RTT (a long/asymmetric path —
                    // e.g. a distant player to a US/EU server — biases the SNTP
                    // offset), THIS probe's raw sample, and how far it pulls from
                    // the running estimate: a distant client's samples jump around
                    // (big devFromEst), a close one's are tight. devFromEst is only
                    // meaningful once synced. Remove once confirmed.
                    long sampleOffset = serverMs - (t1 + (t4 - t1) / 2);
                    long prevEst = _clock.OffsetMs;
                    bool wasSynced = _clock.Synced;
                    _clock.AddSample(t1, serverMs, t4);
                    Log.Info("net.clock",
                        $"time_sync rtt={t4 - t1}ms sample={sampleOffset}ms " +
                        $"devFromEst={(wasSynced ? sampleOffset - prevEst : 0)}ms " +
                        $"smoothed={_clock.OffsetMs}ms synced={_clock.Synced}");
                    break;
                }
                case "offer":
                    OfferReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "answer":
                    AnswerReceived?.Invoke(Str(root, "from"), Str(root, "sdp"));
                    break;
                case "ice_candidate":
                    IceCandidateReceived?.Invoke(
                        Str(root, "from"),
                        Str(root, "candidate"),
                        Str(root, "sdp_mid"),
                        IntProp(root, "sdp_m_line_index"));
                    break;
                default:
                    Log.Debug("net.signaling", $"ignoring unknown frame type '{typeEl.GetString()}'");
                    break;
            }
        }
        catch (Exception e)
        {
            Log.Warn("net.signaling", $"failed to parse frame: {e.Message}");
        }
    }
}
