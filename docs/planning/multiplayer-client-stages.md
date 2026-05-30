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

## Stage 2 — WebRTC peer connection ✅

**Delivers:** `start_signaling` now establishes real peer-to-peer WebRTC
DataChannels. Finish line proven by a ping/pong round-trip in the lobby
("N/N connected · M echo OK"); gameplay over the transport is Stage 3.

**How it was built:**
- **`webrtc-native` GDExtension**, fetched (not committed) by
  `scripts/fetch-webrtc.sh` (pinned `1.1.0-stable`, Godot 4.1+) into
  `client/addons/webrtc/`; CI runs it before `godot --import`. Without it
  `WebRtcPeerConnection` is interface-only.
- **Headless teardown crash:** with the extension loaded, `godot --headless`
  segfaults on *exit* (after the work + artifacts are done). CI wraps the
  headless calls in `scripts/godot-headless.sh`, which forgives the
  SIGSEGV/SIGABRT at exit and relies on the export-verification steps (pck
  size, per-arch DLL, native-lib presence) to catch real failures. The
  extension's editor library is kept so local in-editor play (F5, which
  reports `OS.has_feature("editor")`) still has WebRTC.
- **`IPeerTransport`** (`client/src/net/IPeerTransport.cs`) is the
  topology-agnostic seam the game layer consumes — **topology is a
  per-game-mode strategy**: a future mode can supply a relay/SFU transport
  without touching gameplay. `WebRtcMeshTransport` is the Extended-mode
  implementation (full mesh).
- **Negotiation** rides `SignalingClient` (offer/answer/ice already relayed
  by the server). Glare avoided by a deterministic rule: the
  lexicographically-smaller `player_id` offers and owns the data channel.
- **ICE:** STUN `stun.l.google.com:19302` only. **No TURN** — symmetric-NAT
  peers still can't connect (see Later work). Topology is the `n*(n-1)/2`
  mesh.

**Deferred from Stage 2:** the WS-ticket auth hardening (still cleartext
token in the WS `identify`) and TURN.

---

## Stage 3 — Extended-mode gameplay ✅

**Delivers:** a playable Extended-mode round over the Stage-2 mesh, rendered in
2D, architected so 3D is a later view swap. Full rules + the networking model
are documented in [`../architecture/extended-mode.md`](../architecture/extended-mode.md)
(the canonical picture is `Example Imgs/GameMode Extended.png`).

**The committed model — per-screen, not a shared arena:** each player renders
**only their own screen**; a ball lives on **one screen at a time** and is handed
to a peer when it crosses a shared edge.

```text
GameState  (plain data — no Godot nodes; per screen, multi-ball-ready)
  balls:   [ { id, pos, vel, lastHitter } ]
  paddle:  bottom-anchored (this player's goal); edges: portal(peer) | wall | goal
     |
     v  (observed by)
IGameView  <-- swappable
  ├─ View2D : Node2D    (this stage — render with Vector2)
  └─ View3D : Node3D    (later  — map the same data to Vector3, z fixed)

GameSimulation steps GameState; NetGameController carries handoffs over
IPeerTransport and scores over the signaling socket. Neither touches the view.
```

- **Bottom = your goal** (paddle above it); **top/right/left** are portals to
  peers or trig-reflecting walls. Edge→peer map is built from the actual roster
  at Start (2–4 players; empty slots are walls).
- **Ball handoff:** crossing a portal = a directed `Send` to that one peer; the
  ball re-enters their screen inward via a frame-independent (perp/tang/along)
  transform (`BallTransform`), fast-forwarded by transit time. Because a ball is
  only drawn on one screen, there is **no contested-ball reconciliation and no
  continuous ball-state stream** — simpler than a shared-arena design.
- **Scoring is server-relayed:** the scored-on client reports the last hitter;
  the server holds the authoritative tally and broadcasts `ScoreUpdate`.
  Self-goals don't count. (This is why the score path differs from the older
  P2P sketch in `game-architecture-summary.md`.)
- **Entered from the lobby Start transition** (no solo mode this stage); the
  Stage-2 ping/pong heartbeat was removed.
- **3D later** = implement `View3D` and map `(x, y)` → `Vector3`; simulation and
  networking are untouched.

**Deferred from Stage 3** (see `extended-mode.md`): multi-ball (needs
globally-unique ball ids), solo/AI opponent, a ball-speed cap, and a serve gate
until peers connect.

---

## Stage 4 — Server-authoritative host promotion ✅

**Delivers:** a live match now **survives losing its host**. On an unexpected
host WebSocket drop (past Waiting) the server opens a **30s reconnect grace
window** (`HostReconnecting`); if the host re-Identifies in time it resumes
unchanged (`HostReconnected`), otherwise the next player in chronological join
order is **promoted** (`HostChanged`) — or the session ends if fewer than two
connected players remain. A deliberate mid-game host `Leave` promotes
immediately. Shipped as server **v0.9.0** + game **v0.7.0**.

**How it was built:**
- **Server** (`server/src/signaling/`): host disconnect branches on session
  state. Waiting still tears the lobby down; past Waiting it either promotes (on
  an explicit `Leave`) or arms a grace timer. `SignalHub` holds a cancellable
  per-`(code, host)` grace handle (tokio `oneshot`); the reconnect path and the
  timer race on a single-winner `take_host_grace`, so promotion can't double-fire.
  `promote_or_end_active` is an atomic Lua script that picks the **oldest
  still-connected joiner** (skipping ghosts whose WS is gone) and requires ≥2
  connected players to continue.
- **Client WS reconnect** (`client/src/net/SignalingClient.cs`): the grace window
  is only reachable because the client now **re-dials** a dropped session WS
  (re-sending `identify`) for ~30s instead of bailing to the menu — surfacing
  `Reconnecting`/`Reconnected`. This delivers the deferred "WS reconnect" item
  from [`session-multiplayer-edge-cases.md`](../architecture/session-multiplayer-edge-cases.md).
  Only a deliberate close or an auth-level rejection (4401/4403/4404) is terminal.
- **In-game UI** (`client/src/game/GameScene.cs`): a "Reconnecting…" /
  "Host reconnecting…" overlay; `HostChanged` updates the local host notion;
  Escape leaves the match deliberately. The ball keeps flowing over the
  independent WebRTC mesh while the WS reconnects.
- **Joiner roster cleanup** folded in: an explicit joiner `Leave` past Waiting
  now frees the slot and ends the session if the host is left alone (the
  previously-deferred "Joiner WS disconnect during Starting/Active" concern). A
  transient joiner drop keeps the slot for reconnect.

Join order was already tracked (`JoinerEntry.joined_at_ms`); the Stage-1
voluntary transfer puts a demoted host at the back of that order, and promotion
consumes the front.

**Deferred from Stage 4:**
- **Grace windows are constants** (`PROMOTION_GRACE` / `RECONNECT_GRACE`), not
  runtime config.
- ~~Process-death recovery~~ — **delivered in Stage 5.**
- ~~Host reconnect after promotion~~ — **delivered in Stage 5** (promotion now
  demotes the ex-host instead of removing them; they rejoin as a non-host).

---

## Stage 5 — Process-death recovery + uniform reconnect window ✅

**Delivers:** a player whose game **process fully dies** mid-match (not just a
transient WS blip) can get back into the **same live match** by **manually
re-entering the session code**; the WebRTC mesh re-establishes so balls flow
again. Shipped as server **v0.10.0** + game **v0.8.0**.

**The model — one window for everyone, measured from the drop:**
- Any dropped mid-game player gets the **same reconnect window** (`RECONNECT_GRACE`,
  120s): peers see a 30s "reconnecting…" overlay (`HostReconnecting` for the host,
  the new `PeerReconnecting` for a joiner), the Redis slot is held for the full
  window, then **freed permanently** (`PeerLeft { reconnect_timeout }`).
- The **only** host difference: a 30s **promotion** sub-timer inside that window.
  At 30s with no return, the oldest connected joiner is promoted (`HostChanged`)
  and the **ex-host is demoted into `joiners` (kept, not removed)** — so they keep
  the rest of their window and rejoin **as a non-host**.

**How it was built:**
- **Server** (`signaling/`): the grace registry gains a `GraceKind`
  (`Promotion` / `Reconnect`); `arm_grace`/`take_grace(kind)` generalise the
  Stage-4 host-grace single-winner. A transient mid-game drop arms the reconnect
  slot-hold (+ overlay); the host additionally arms the promotion timer.
  `promote_demote_or_end_active` appends the ex-host to `joiners` on a transient
  drop (`keep_ex_host`). Re-Identify takes the relevant grace(s).
- **Client rejoin** (`ui/menus/JoinMenu.cs`, `core/SessionContext.cs`): entering
  an already-started session's code that you belong to re-opens the WS, rebuilds
  the mesh, and enters `GameScene` (mirrors the lobby's start transition);
  `4403/4404` → friendly rejection.
- **Client re-mesh** (`net/IPeerTransport.ResyncPeer`, `net/WebRtcMeshTransport`,
  `game/net/NetGameController`): a returning peer's walled-off portal is healed
  and just that one connection re-negotiated. A small in-game **session-code
  label** lets players reshare the code.

**Deferred from Stage 5:**
- **Ball-loss recovery:** if the single ball died with the crashed process, the
  rejoined match has no ball until a watchdog re-serves it. Design: the holder
  broadcasts a `BallAlive` heartbeat over the mesh; after a gap the lowest-id
  connected player serves one. Fast-follow.
- **Grace windows as runtime config** (still constants).

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
