# Multiplayer client — staged build order

This document lays out the planned order for bringing the **game client**
from "menu shell" to "playable networked multiplayer," and records the
notes that make each later stage easier to start. It exists because the
work is large and was deliberately broken into stages: each stage ships a
coherent, testable slice, and the riskiest piece (Godot's WebRTC) is
pushed back until the lobby flow around it is proven.

The **server** side of all of this already exists and is documented in
[`protocol.md`](../architecture/protocol.md),
[`game-architecture-summary.md`](../architecture/game-architecture-summary.md),
and [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md).
This file is about the **client** catching up to it.

## The three-layer mental model (why this is safe to stage)

Treat the game as three independent layers:

1. **Transport** — moving bytes (HTTP matchmaking, WebSocket signaling,
   WebRTC gameplay). Renderer-agnostic: a position on the wire is just
   numbers.
2. **Simulation** — the authoritative game rules (ball position, scores,
   turns). The "truth."
3. **Presentation** — what the player sees: 2D sprites (`Node2D`/`Vector2`)
   or 3D meshes (`Node3D`/`Vector3`).

Because transport and simulation don't care about 2D vs 3D, **the
networking built in Stage 1–2 is never wasted** regardless of how
rendering evolves. The one rule that keeps "2D now, 3D later" cheap:
**never fuse simulation into presentation** — the simulation owns state as
plain data, and a swappable *view* draws it.

---

## Stage 1 — Lobby foundation ✅ (this branch)

**Delivers:** a fully working multiplayer lobby over the live server.

- Launcher hands the client its channel identity (`player_id`,
  `secret_token`), its own version, and the channel
  (`launcher/src/game_launch/mod.rs` → `client/src/core/LaunchArgs.cs`).
- Client networking layer (`client/src/net/`): `ServerApi` (HTTP) and
  `SignalingClient` (WebSocket), with DTOs mirroring `shared/`.
- Host / Join / Lobby menus call the real endpoints. The lobby roster is
  driven by signaling events (`Identified`, `PeerJoined`, `PeerLeft`,
  `HostChanged`), with manual host handoff (`POST /session/:code/host`),
  Start, Cancel/Leave, and `SessionEnded`/`Kicked`/disconnect handling.
- Server companions added this branch: the host-transfer endpoint +
  `HostChanged`, freeing a joiner's slot on explicit leave during Waiting,
  and `host_player_id` in the `Identified` frame.

**Stops at:** the moment `start_signaling` arrives. The lobby shows "all
players ready"; no peer connection is made yet.

**Known Stage 1 limitations (deferred):**
- Peers display as `Player <id>` — the server roster carries no usernames.
- Running from the editor (no launcher handoff) uses a DEBUG/editor-only
  self-register fallback to get an identity; it doesn't exist in release
  builds. The server host is always the compile-time-baked channel host.

---

## Stage 2 — WebRTC signaling + peer connection

**Goal:** turn `start_signaling` into actual peer-to-peer connections.

**Notes for crossing this bridge:**
- **Godot 4 C# caveat:** `WebRtcPeerConnection` in the core API is an
  *interface only* — establishing real connections requires the
  `webrtc-native` GDExtension (the `webrtc` addon) added to the project and
  shipped in every export. Budget time for getting that building on all
  three platforms (and the macOS universal export).
- The wire shape already exists: `ClientMsg::{Offer, Answer, IceCandidate,
  PeerConnectionFailed}` and the matching `ServerMsg` relays
  (`server/src/signaling/protocol.rs`). `SignalingClient` already receives
  `offer`/`answer`/`ice_candidate` and currently just logs them — that's
  where negotiation hooks in.
- **ICE servers:** STUN `stun.l.google.com:19302` for now. TURN is a later
  item (see Later work) — symmetric-NAT peers can't connect until then.
- **Topology:** every pair negotiates its own offer/answer/ICE, so `n`
  players form an `n*(n-1)/2` mesh.
- This is the right moment to adopt the WS-ticket auth pattern (Later work)
  — define it once for the client at the same time.

---

## Stage 3 — Ball simulation + the swappable view (2D now)

**Goal:** an actual playable round, rendered in 2D, architected so 3D is a
later view swap rather than a rewrite.

**The sim-on-plane architecture (committed decision):**

```text
GameState  (plain data — no Godot nodes)
  ball:    { x, y, vx, vy }
  paddles: [ ... ]
     |
     v  (observed by)
IGameView  <-- swappable
  ├─ View2D : Node2D    (Stage 3 — render with Vector2)
  └─ View3D : Node3D    (later  — map the same data to Vector3, z fixed)

Transport/Simulation talk to GameState, never to the view.
```

- Keep authoritative ball/paddle state as plain data on an abstract 2D
  plane. The view *observes* and draws; it never owns the truth.
- Networking writes peer updates into `GameState`; the view reads from it.
- Apply the sync techniques already designed in
  [`game-architecture-summary.md`](../architecture/game-architecture-summary.md):
  **dead reckoning** (keep simulating from last known velocity during
  packet delay) and **reconciliation** (lerp small corrections over a few
  frames, snap large ones).
- Adding 3D later = implement `View3D` and map `(x, y)` → `Vector3`. The
  simulation and networking are untouched.

---

## Stage 4 — Server-authoritative host promotion

**Goal:** automatic promotion when the host is lost unexpectedly (the
deferred half of promotion; Stage 1 only does *voluntary* handoff).

**Notes:**
- The design is in `game-architecture-summary.md` ("Host Promotion Queue"):
  on host disconnect during an active session, promote the next player in
  chronological join order; if only one player remains, end the session.
- Join order is already tracked (`JoinerEntry.joined_at_ms`). The voluntary
  transfer added in Stage 1 puts a demoted host at the back of that order.
- Touches `server/src/signaling/ws.rs` (the disconnect path currently only
  mutates the SignalHub past Waiting — see the "Joiner WS disconnect during
  Starting/Active" entry in
  [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md))
  and adds a broadcast the client reacts to (reuse `HostChanged`).

---

## Later work (not yet scheduled)

Tracked in or alongside [`roadmap.md`](roadmap.md):

- **TURN relay** — so symmetric-NAT players can connect (Stage 2 ships
  STUN-only).
- **Score validation** — server-side trajectory checks (needs Stage 3
  scoring to exist first).
- **WS-ticket auth** — replace the cleartext `secret_token` in the WS
  `identify` frame with a short-lived signed ticket. Good to define when
  Stage 2 lands.
- **Usernames in the session roster** — so the lobby shows names instead of
  `Player <id>`. Needs the server to store a username per session member.
- **3D view** — `View3D` per the Stage 3 architecture.
