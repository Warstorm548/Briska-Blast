# Session multiplayer — edge cases and open questions

This file captures known-but-unaddressed edge cases and design questions
from the introduction of N-player sessions, the `/start` endpoint, and
WebSocket signaling. It exists so these items survive past plan
approval and have a durable home for follow-up work.

Treat the **Edge cases** section as a checklist of behaviors the server
exhibits today and whether they're considered correct. Treat the **Open
questions** section as decisions the project has chosen to defer; each
records the current default and what would have to change.

Last reviewed: server v0.10.0 — Stage 5 process-death rejoin + uniform reconnect window.

---

## Edge cases

### WS auth replay risk

`Identify` carries the `secret_token` as cleartext over the WebSocket.
On a localhost dev box this is fine. In production the WS must run over
TLS (`wss://`) — anything less leaks the token to anyone on the path.

**Future hardening:** issue a short-lived signed WS ticket from
`/host` / `/join` responses. The launcher then opens the WS with
`?ticket=<jwt-like-thing>` instead of replaying the raw token. Server
verifies the ticket signature + expiry, never sees the secret on the
wire. See the [WS-ticket auth roadmap entry](../planning/roadmap.md).

### Concurrent `/join` race

Two simultaneous `/join` calls against a session with one remaining
slot must NOT both succeed. The pre-refactor load-modify-store JSON
pattern was vulnerable: both joiners could read the same "1 slot left"
state and both write back successfully.

**Current fix (server v0.5.0, `server/src/api/join.rs`):** the entire
read-check-append-write happens inside a single Redis Lua script.
Redis Lua runs to completion against the keyspace before yielding, so
two concurrent calls serialize cleanly.

Integration coverage for this lives in the integration test suite that
ships with the validation work; if anyone changes the join path,
re-run `concurrent_join.rs`.

### WS reconnect

**Now implemented client-side (game v0.7.0, Stage 4).** When a session WS drops
unexpectedly, the Godot client re-dials the same `/ws/session/{code}` and
re-sends `Identify` for ~30s before giving up (only a deliberate close or an
auth-level 4401/4403/4404 is terminal). This is what makes the host-reconnect
grace below reachable. The server still has no idempotency token —
re-Identifying with the same `player_id` is treated as a duplicate identify
(see below) — but when a *host* returns within its grace window, the reconnect
path additionally cancels the pending promotion timer (`take_grace`).

**Process-death rejoin (Stage 5, game v0.8.0 / server v0.10.0).** A *transient
WS blip* (process alive) is recovered automatically by the above re-dial. A
**full process death** is recovered *manually*: the player re-enters the session
code on the Join screen, which re-Identifies them into the still-held slot and
re-establishes the WebRTC mesh (see the disconnect sections below for the slot
hold). The slow re-dial isn't automatic because the relaunched process has lost
its in-memory session state.

### Host WS disconnect during Waiting

Host loses their WS while the session is still in `Waiting`. Without
intervention, joiners would sit on a session that has no host and no
way to start.

**Current behavior (`server/src/signaling/ws.rs::end_session_if_waiting`):**
on host disconnect, the server checks the session status. If it's
still Waiting, the Redis session key is deleted and `SessionEnded
{ reason: "host_disconnect" }` is broadcast to remaining peers. Their
WS handlers see the broadcast and propagate it to clients.

### Host WS disconnect during Starting / Active

**Implemented in Stage 4, extended in Stage 5 (server v0.10.0).** Past Waiting the
session must survive host loss. On the host's WS dropping, the server broadcasts
`HostReconnecting { grace_secs: 30 }` and arms **two** timers
(`ws.rs::arm_host_disconnect_grace`): a 30s `Promotion` timer and the uniform
`RECONNECT_GRACE` (120s) slot-hold. If the host re-Identifies before promotion,
the reconnect path cancels both and broadcasts `HostReconnected`. Otherwise at
30s `promote_demote_or_end_active` promotes the oldest **still-connected** joiner
(`HostChanged`) — or ends the session if fewer than two connected players remain
(`SessionEnded { reason: "host_disconnect" }`). **Stage 5 change:** on a transient
drop the promotion now **demotes the ex-host into `joiners`** (kept, not removed),
so they keep the rest of their 120s window and can rejoin **as a non-host**. A
deliberate mid-game host `Leave` skips the grace, promotes immediately, and drops
the ex-host. The grace registry's single-winner `take_grace` guarantees promotion
can't race a reconnect.

### Joiner WS disconnect during Waiting

The server distinguishes a **deliberate leave** (the client sent a
`Leave` frame) from a **transient drop** (the socket just died):

- **Transient drop:** the joiner's entry stays in `Session.joiners` so
  capacity accounting is unaffected, and `PeerLeft { reason: "disconnect" }`
  broadcasts to the lobby. The joiner can re-`Identify` on a new WS to
  rejoin signaling. `/start` refuses to transition while their WS is
  missing (the `not_all_peers_ready` precondition), so the host either
  waits for them or asks them to leave.
- **Deliberate leave (Waiting only):** `ws.rs::remove_joiner_if_waiting`
  removes the joiner from `Session.joiners` in Redis via a single Lua
  script, and `PeerLeft { reason: "leave" }` broadcasts. This frees the
  slot so the lobby capacity is correct and `/start`'s "all peers ready"
  check can pass for the remaining members. (Without this, a player who
  left would keep their slot until TTL and permanently block `/start`.)

A leave **past Waiting** (Starting/Active) still only broadcasts
`PeerLeft` and removes the SignalHub sender — the Redis roster mutation
there is the deferred auto-promotion concern (see below).

### Joiner WS disconnect during Starting / Active

**Updated in Stage 4 (server v0.9.0).** The server now distinguishes the two
cases, mirroring the Waiting semantics:

- **Explicit `Leave`:** `remove_joiner_on_leave` frees the slot in Redis (one
  atomic Lua script, so it can't corrupt a concurrent `/start` write) and ends
  the session if the leave empties the roster, leaving the host alone
  (`SessionEnded { reason: "last_player_left" }`). `GET /session/<code>` is now
  accurate after a deliberate mid-game leave.
- **Transient drop (updated in Stage 5):** the server arms the uniform
  `RECONNECT_GRACE` (120s) slot-hold and broadcasts `PeerReconnecting` (peers show
  a "reconnecting…" overlay) instead of an immediate `PeerLeft`. Remaining peers
  wall off the departed portal client-side (`NetGameController.OnPeerLost`) until
  they return. The player can rejoin within the window (auto re-dial for a WS
  blip, or manual code re-entry after a process death) — `PeerJoined` then heals
  the mesh (`OnPeerRejoined` → `ResyncPeer`). If the window elapses the slot is
  freed for good (`PeerLeft { reason: "reconnect_timeout" }`), so `GET
  /session/<code>` stops listing them. (Previously the slot was kept until the
  session TTL.)

### WebRTC fails for one peer to all others (symmetric NAT)

A symmetric-NAT player can complete `Identify` and receive
`start_signaling`, but every peer pair fails to establish ICE
connectivity. Without a TURN relay, this player cannot participate.

**Current behavior:** their client reports `peer_connection_failed`
to the server for each peer. The server logs it. Full kick semantics
(broadcast `PeerLeft`, send `Kicked { reason: "peer_connection_unrecoverable" }`,
close socket) is wired but the trigger threshold is not yet
implemented — every-pair-failed detection is the next step.

**Future:** TURN relay so this group can play. See the [TURN relay
roadmap entry](../planning/roadmap.md).

### Session TTL expires mid-flight

Redis sessions have a TTL (default 1800s) refreshed on each `/host`
and `/join` write. If a lobby sits idle past the TTL, the key
disappears. Subsequent HTTP calls return 404; the WS handler still
holds a live connection but no longer has a session to route against.

**Current behavior:** WS frames continue to be relayed (the SignalHub
doesn't know the session is gone). A new join attempt to the dead
code 404s. We rely on TTL being long enough to outlast any plausible
lobby duration. Sweep-on-disconnect is a candidate hardening.

### Server restart mid-signaling

Per-process SignalHub state is lost. All WS clients see clean Close
frames. The Redis session JSON persists if the TTL hasn't passed.
Clients reconnect WS via the standard reconnect path and re-`Identify`;
the room reforms in the new process.

If Redis lost the session (TTL hit during downtime), `Identify` 4404s
and the launcher falls back to the lobby selection screen.

### Stale empty rooms in SignalHub

A `Room` whose `senders` map is empty must not remain in `SignalHub.rooms`
forever. `leave_room` removes the room when the last sender leaves;
this is verified by `leave_room_drops_empty_room` in the SignalHub unit
tests.

### Duplicate `Identify` (same player_id on a new WS)

A second WS connection for an already-connected player sends `Identify`.

**Current policy:** the second `Identify` is ignored — both sockets stay
open. This is a placeholder; the intended policy is "kick old": send
`Kicked { reason: "replaced" }` to the old socket, close it, accept the
new one. Tracked under Open Questions.

### `player_count == 1`

A bounds entry with `min_players = 1` would allow a single-player
session. The `/start` precondition `current_player_count >= min`
passes, but there are no peers to signal to — `start_signaling`
broadcasts to a roster of one (the host themselves).

This isn't broken — it's just not what a multiplayer game wants. If a
practice / solo mode is ever added, it should either skip the WebRTC
phase entirely or run a self-loopback connection. For now, no
gamemode declares `min_players = 1`.

### WS message size cap

SDP bodies are typically 2–5 KB; ICE candidate frames are tiny. To
defend against pathological frames (a malicious client trying to OOM
the server), the WS reader should cap frame size at 64 KB. Currently
relies on tungstenite's default (`max_message_size = 64 MiB`). Tighten
when production traffic exists.

---

## Open questions

Decisions the project has chosen to defer, with the current default
behavior and the trigger that would force us to revisit.

### 1. Additional `GameMode` variants beyond `Extended`

**Current:** `Extended` only, with bounds (2, 4) hardcoded in
`server/src/gamemode.rs`.

**Trigger to revisit:** more gamemodes designed. Adding a variant is
two lines (one in `shared/src/types/gamemode.rs`, one in
`server/src/gamemode.rs::bounds_for`) plus tests. The `bounds_for`
match is exhaustive without a wildcard — forgetting the bounds row
is a compile error.

### 2. `/start` strictness — all WS-identified, or allow gaps?

**Current default:** strict. Every session member must have an active
WS connection identified in SignalHub before `/start` succeeds.
Otherwise `SessionNotStartable { reason: "not_all_peers_ready" }`.

**Trade-off:** strict is the cleanest UX (no zombie sessions
mid-signaling) but means a single flaky player's WS issue stalls the
whole lobby. A relaxed policy ("start anyway, drop missing peers")
might be friendlier when networks are bad.

**Trigger to revisit:** real-world reports of stalled `/start` calls
where the missing peer is the obvious cause.

### 3. Duplicate `Identify` policy — kick old or reject new?

**Current default:** ignore the second `Identify` (no-op). Both
sockets remain open.

**Intended:** kick the old socket with `Kicked { reason: "replaced" }`
on duplicate identify. Mirrors typical chat semantics — last writer
wins.

**Trigger to revisit:** the launcher needs WS reconnect-while-old-is-stuck
behavior to feel sane, which means we need duplicate-identify kick.

### 4. Symmetric-NAT detection threshold

**Current default:** every peer pair must fail before a peer is
considered unrecoverable. Server logs each `peer_connection_failed`
but doesn't kick yet.

**Alternative:** majority of pairs failed — kick on a 3/3-fails-out-of-4
basis. Faster reaction but risks kicking peers whose ICE just hadn't
completed.

**Trigger to revisit:** the TURN relay work lands; the threshold is
the gate that decides "fall back to TURN" vs "give up."

### 5. `/test/webrtc` exposure in production

**Current default:** off. The route is only registered when the env
var `ENABLE_TEST_HARNESS=true`. A warn-level log fires at startup
if it's enabled.

**Possible future:** ship the harness behind a feature flag and an
admin-panel-only auth gate so an operator can verify a deployment
from a browser. Keep off by default forever otherwise.

**Trigger to revisit:** a real operational need for browser-side
diagnostics in production.

### 6. WS-ticket auth pattern adoption

**Current default:** `secret_token` cleartext over the WS, mitigated
only by `wss://` in production.

**Future:** server issues a short-lived signed WS ticket from
`/host` / `/join` responses; launcher uses the ticket on WS open
instead of the raw token.

**Trigger to revisit:** security audit, or any production deployment
that runs WS over a path the operator doesn't fully trust.
