# Extended game mode — rules & networking model

Extended is the only game mode (2–4 players; `shared/src/types/gamemode.rs`,
`server/src/gamemode.rs`). This document is the reference for how a round plays
and how the ball moves between players. The canonical picture is
`Example Imgs/GameMode Extended.png`.

It complements the staged build history in
[`../planning/multiplayer-client-stages.md`](../planning/multiplayer-client-stages.md)
and the networking overview in
[`game-architecture-summary.md`](game-architecture-summary.md).

## The core idea: every player has their own screen

There is **no shared arena**. Each player renders **only their own screen**, and
a ball lives on **exactly one screen at a time** — the screen it is currently on
simulates it authoritatively. The ball travels between players by crossing an
edge that is shared with another player (a "portal").

```text
        a player's own screen (always drawn upright)

        ┌──────────── TOP ────────────┐
        │                             │      TOP / LEFT / RIGHT are PORTAL edges:
       LEFT          (ball)         RIGHT       • peer present  → ball crossing is
        │                             │           HANDED OFF to that one peer
        │        ▭ paddle ▭          │         • peer absent   → solid WALL (reflect)
        └─────────── BOTTOM ──────────┘
                 (goal, with a gap)        BOTTOM is always YOUR goal. The paddle
                                           sits above it and slides left/right.
```

- **Bottom = your goal.** Your paddle moves horizontally above it with a small
  gap. If a ball gets past your paddle and crosses the bottom line, the rally
  ends and a point may be scored (see Scoring).
- **Top / Left / Right = portals or walls.** Each is either shared with one peer
  (a portal) or, if no peer occupies that slot, a solid wall.

## Topology: a full mesh of shared edges

Every pair of players shares exactly one edge, so for 4 players there are
`4·3/2 = 6` shared edges — matching the WebRTC full mesh (`WebRtcMeshTransport`).
Each player uses up to **3 portal edges + 1 goal edge**. Player count is dynamic:
the host configures a max of 2–4 but may Start early, and the edge→peer map is
built from the **actual roster at Start**. Unfilled portal slots are walls.

- 2 players → 1 portal + 2 walls + goal (≈ classic Pong)
- 3 players → 2 portals + 1 wall + goal
- 4 players → 3 portals + goal

Edges are assigned **by seat**, matching the canonical image. Players sit at a
fixed table — **P1 (bottom), P2 top, P3 left, P4 right** — in **join order**: P1
is the player who created the lobby (the start-time host, first to join), then the
rest in the order they joined. On your own upright screen the peer seated
**opposite** you takes your **Top** edge, the one on your **right hand** takes
**Right**, and the one on your **left** takes **Left**; any seat with no player is
a wall. This reproduces the per-player layout in the image (e.g. P3 sees P4 on
Top, P1 on Right, P2 on Left) rather than the same shape for everyone.

The seating order is **frozen server-side at Start**: the `/start` handler
snapshots `[host, …joiners]` into `session.seat_order` (server ≥ v0.17.0) and
never mutates it. The client captures it once — from the `start_signaling`
roster on a fresh start, or from the `Identified` frame's `seat_order` on a
process-death rejoin — into `SessionContext.SeatOrder`, and
`GameScene.BuildEdges` reads it a single time in `_Ready`. Because the snapshot is
frozen, the layout is identical on every client and **never changes mid-match**:
if the host disconnects and a peer is promoted, no one is re-seated and no portal
moves — a dropped peer's edge only toggles to a wall and is restored to the
**same** edge if they rejoin (`NetGameController`), and a rejoiner reconstructs the
identical seating even if a promotion happened while it was gone. (Against an
older server with no `seat_order`, the client falls back to a deterministic
host-first + `player_id` sort.) Unfilled portal edges are walls, so the cases
above still hold: 2 players → 1 portal (Top) + 2 walls; 3 → 2 portals + 1 wall;
4 → 3 portals.

## Ball travel (handoff)

When a ball crosses a portal edge it is **handed directly to that one peer** (a
directed send, never a broadcast) and removed from the sender's screen. It then
appears on the receiver's screen entering inward from the edge the receiver
assigned to the sender.

Because each player renders only their own screen, the two ends don't share a
coordinate frame. The handoff carries a **frame-independent canonical form** —
the speed perpendicular to the crossed edge, the speed tangential to it, and the
position along it `[0,1]` — which the receiver maps onto its own entry edge with
the perpendicular component now pointing inward (speed is preserved). The packet
also carries a send timestamp **in a server-synced time frame** (each client
estimates its offset to the server clock via a `time_sync` probe on the session
WebSocket — `client/src/net/ServerClock.cs`); the receiver **fast-forwards** the
ball by the transit time so it doesn't visually lag at entry. Stamping both ends
in the shared frame keeps that transit free of the two machines' wall-clock skew
(which otherwise drifts apart and, after a while, drops the ball partway down the
receiver's screen). Because a ball is only ever
drawn on one screen, there is **no continuous ball-state stream and no
reconciliation** — the only timing artifact is the handoff gap.

## Collisions

All collisions use trigonometric angle reflection:

- **Walls** (and any unoccupied non-goal edge): the velocity component normal to
  that axis-aligned edge is negated; speed is preserved.
- **Paddle:** where the ball strikes relative to the paddle centre steers the
  outgoing angle (centre → straight up; the edges → up to ±60° off vertical),
  speed preserved. This is the classic paddle "english."
- **Corner barriers:** solid obstacles in the four corners (below); a ball bounces
  off the least-penetrated face, negating that velocity component.

### Corner barriers

A solid **L-shaped barrier** sits in **all four corners of every screen** (identical
on every client — it's static local geometry, never networked). It reduces cheap
scoring in the goal corners and stops fast balls from cutting the corners.

- **Placement.** The `Cornerbarrier` sprite is drawn for the **bottom-left** corner;
  the other three are the *same* sprite rotated in 90° steps about its **bottom-left
  pixel**, with that pixel pinned to the screen corner. Every L opens toward the arena
  centre. Size is a fraction of arena **height** (`CornerBarrier.HeightHFrac`), like the
  paddle / ball / goal-gap tuning, so every resolution gets the same proportion.
- **Collision model.** Each L is its two solid bars — a vertical **arm** and a
  horizontal **foot** — so a barrier is 2 axis-aligned rects, 8 across the four corners.
  The transparent inner notch of the L is left open (the ball can occupy it). A ball is
  resolved against each rect (Minkowski-inflated by the ball radius) **before** the
  goal/edge checks, so a ball entering a bottom goal corner is turned away instead of
  slipping past the paddle. Position-based like the wall/goal checks (no sweep).
- **Single source of truth.** `CornerBarrier` (`game/CornerBarrier.cs`) owns the corner
  → (pivot, rotation, scale) table and the local L rects. The simulation derives its
  collision rects and the view places its sprites from that same helper, so the collider
  can never drift from the art. The barrier is registered as `AssetId.CornerBarrier` with
  the `AssetCategory.SystemControlled` tag (game-owned but static — not a tunable random
  spawn, so it never appears in the host's Random-Spawns UI).

## Scoring (server-relayed)

- A point goes to the **last player to have applied force to the ball**, awarded
  when the ball passes a paddle into that player's goal. Applying force means
  either deflecting it with your paddle **or serving it** — a serve tags the ball
  with the serving player's id the instant it launches, exactly as a paddle hit
  does, and a later hit by anyone overwrites it. So a clean serve that crosses
  untouched into a peer's goal scores for the player who served.
- **A self-goal does not count:** if the scored-on player is also the last to
  have applied force, no point is awarded (e.g. your own serve bouncing back into
  your own goal untouched). Because every ball is served, a truly *untouched*
  ball no longer arises in practice; the "no last hitter ⇒ nobody scores" guard
  remains only as a defensive fallback.
- Scoring is **relayed through the server**, not peer-to-peer: the client whose
  goal the ball entered reports the scorer over the session WebSocket
  (`ReportScore`); the server holds the **authoritative** per-session tally and
  broadcasts `ScoreUpdate` to every client, which overwrites its local
  scoreboard (never increments locally — a dropped/duplicated frame can't
  desync). The server credits only players currently in the room. This is the
  hook for later server-side trajectory validation.
- **Point value travels with the report.** A normal goal is worth 1; a **BallBT
  split ball** is worth 2 (`ReportScore.points`, server-clamped to `[1, 2]`). The
  server adds the reported points to the authoritative tally.

## Win condition & game over

- The host picks a **win condition** during setup (Advanced → Match Rules),
  mirroring `gamemode`: currently **"Set Score"** — first player to a target score
  (range 10–50, **default 11**) wins. It is required, range-validated server-side
  (`invalid_win_condition`), broadcast to joiners (join/poll responses +
  `start_signaling`), and seeded into the room's in-memory tally at `/start`.
- When an accepted `ReportScore` first makes a player's tally reach the target, the
  server broadcasts a dedicated **`GameOver { winner_player_id, scores }`** frame.
  This is a **pure UI signal**: every client freezes its simulation and shows the
  end-game leaderboard (all players, winner highlighted) with *Return to Main Menu*
  / *Host Game*. Win detection latches, so a late/duplicate report can't re-fire it.
- **Cleanup reuses the existing `SessionEnded` path** rather than a parallel one:
  right after `GameOver`, the server calls the shared `end_session(code, "game_over")`
  (Redis `DEL` + broadcast `SessionEnded`). The frames ride the same ordered
  channel, so each client latches game-over before the `SessionEnded` arrives and
  suppresses its usual auto-leave — the end screen owns navigation from then on.

## Random spawns: ball splitter & BallBT

- A **BallSpliter** is a *system-spawned* element (not player-controlled). Each
  player's screen spawns its own on a host-configured cadence — a cooldown that
  doubles as the respawn timer — at a random spot in the play area. Splitters are
  **local to a screen** and never handed off.
- When the **master ball** touches a splitter, the splitter is consumed and spawns
  **3 BallBT split balls** fanned **45° apart**, centred opposite the master's
  heading so they clear its forward path. The master **passes through unaffected**
  (same colour, same trajectory). Each split ball inherits the master's last-hitter
  as its owner.
- **Chain-splitting** (a split ball hitting another splitter splits again) is a host
  toggle, **on by default**; with it off, only the master ball can trigger a split.
- **BallBT split balls** follow the same last-hitter possession rule, are worth
  **2 points** at a goal (vs 1), and **vanish** when they reach a goal — unlike the
  master, which the scored-on player re-serves. They hand off between screens like
  any ball; the ball **kind** travels in the handoff packet so a split ball stays a
  split ball on the peer's screen.
- Cadence + chain-split come from the host's **Random Spawns** advanced settings
  (`SpawnSettings`: splitter interval 5–60s / default 15, chain-split on), sent at
  `/host`, validated server-side (`invalid_spawn_settings`), and broadcast to every
  client (joiners included) via `start_signaling`, so the rules match across the
  table. Each client drives its **own** local spawner from them.
- Sprites resolve through a central **`SpriteRegistry`** autoload
  (`client/src/core/SpriteRegistry.cs`): every fast-lookup sprite has a stable,
  upward-counting `AssetId` plus a `PlayerControlled` / `SystemHandled` tag. See
  [`asset-registry.md`](asset-registry.md).

## Serve

- **First serve:** at game start, **only the host** spawns a ball resting on its
  paddle; everyone else starts empty (a ball reaches them via a handoff, or when
  they're scored on).
- **After a goal:** when the **master** ball is lost, the scored-on player respawns
  the next master ball on their own paddle and serves it — **always**, even when no
  point was awarded (so a self-goal still hands them the serve). A lost **BallBT
  split ball** is *not* re-served — it just vanishes.
- A resting ball follows the paddle until the player presses serve, which
  launches it.

## Controls (defaults)

- **Left / Right arrow keys** — move the paddle.
- **Space** — serve.

Bound via `physical_keycode` (layout-independent) in `client/project.godot`.
Re-binding through settings is future work.

## Where it lives in code

| Concern | File |
|---|---|
| Per-screen plain-data state | `client/src/game/GameState.cs` |
| Rules (integrate, reflect, score, emit handoff/score events) | `client/src/game/GameSimulation.cs` |
| Frame-independent handoff math | `client/src/game/BallTransform.cs` |
| 2D rendering (swappable view) | `client/src/game/view/View2D.cs` (`IGameView`) |
| Central sprite/asset lookup | `client/src/core/SpriteRegistry.cs` (see [`asset-registry.md`](asset-registry.md)) |
| Corner-barrier layout (shared collision + sprite geometry) | `client/src/game/CornerBarrier.cs` |
| Scene host: input, serve, sim loop | `client/src/game/GameScene.cs` |
| Session lifecycle (lobby → Preparing → match, rejoin, teardown) | `client/src/core/MatchFlow.cs` (see [`match-lifecycle.md`](match-lifecycle.md)) |
| Handoff packet wire format | `client/src/game/net/GamePacket.cs` |
| Net glue (handoff/score over `IPeerTransport` + signaling) | `client/src/game/net/NetGameController.cs` |
| Server score channel | `server/src/signaling/{protocol,ws,mod}.rs` |

## Deferred / future

- **Multiple balls** at once — **shipped** in game v0.18.0 with the ball-splitter
  mechanic: ball ids are seat-namespaced for global uniqueness, the serve no longer
  clears the list, and the sim always steps so concurrent balls keep moving.
- **Solo vs computer** (an AI paddle) — not built; the Solo Play menu button is
  intentionally disabled.
- **Ball-speed cap** — speed is currently preserved on every bounce with no cap;
  a cap is wanted so the ball can never get too fast for the eye to track.
- ~~**Serve gate** — if a player serves before the WebRTC channel is open, the
  first handoff packet is dropped silently; a "wait for peers" gate is wanted.~~
  **Done (game v0.23.0, client-side):** `MatchFlow` holds everyone on a
  "Connecting to players…" screen until every peer data channel is open (30s
  deadline), so `GameScene` — and with it the host's serve — only exists once
  handoffs can flow. See
  [`match-lifecycle.md`](match-lifecycle.md); the server-authoritative half
  (ready barrier) is the planned Stage B.
- **Server-side trajectory validation** and **TURN relay** (symmetric-NAT peers)
  remain in [`../planning/roadmap.md`](../planning/roadmap.md). (**Usernames in the
  roster** shipped in game v0.12.0 / server v0.15.0 — the lobby and scoreboard now
  label players by name.)
- **3D view** — implement `View3D : Node3D` against `IGameView`, mapping the same
  `(x, y)` data to `Vector3`. Simulation and networking are untouched.
