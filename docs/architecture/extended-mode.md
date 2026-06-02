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

Edges are assigned locally: present peers (sorted by `player_id` for
determinism) take Top, then Right, then Left.

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

## Scoring (server-relayed)

- A point goes to the **last player to have hit the ball with their paddle**,
  awarded when the ball passes a paddle into that player's goal.
- **A self-goal does not count:** if the scored-on player is also the last
  hitter, no point is awarded. A ball that reached a goal untouched scores
  nobody either.
- Scoring is **relayed through the server**, not peer-to-peer: the client whose
  goal the ball entered reports the scorer over the session WebSocket
  (`ReportScore`); the server holds the **authoritative** per-session tally and
  broadcasts `ScoreUpdate` to every client, which overwrites its local
  scoreboard (never increments locally — a dropped/duplicated frame can't
  desync). The server credits only players currently in the room. This is the
  hook for later server-side trajectory validation.

## Serve

- **First serve:** at game start, **only the host** spawns a ball resting on its
  paddle; everyone else starts empty (a ball reaches them via a handoff, or when
  they're scored on).
- **After a goal:** the scored-on player respawns the next ball on their own
  paddle and serves it — **always**, whether or not a point was awarded (so a
  self-goal still hands them the serve).
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
| Scene host: input, serve, sim loop | `client/src/game/GameScene.cs` |
| Handoff packet wire format | `client/src/game/net/GamePacket.cs` |
| Net glue (handoff/score over `IPeerTransport` + signaling) | `client/src/game/net/NetGameController.cs` |
| Server score channel | `server/src/signaling/{protocol,ws,mod}.rs` |

## Deferred / future

- **Multiple balls** at once. `GameState` already holds a `List<Ball>` and
  packets carry ball ids; the hard part to design is **globally-unique ball ids**
  once several balls (and ball types) cross between screens.
- **Solo vs computer** (an AI paddle) — not built; the Solo Play menu button is
  intentionally disabled.
- **Ball-speed cap** — speed is currently preserved on every bounce with no cap;
  a cap is wanted so the ball can never get too fast for the eye to track.
- **Serve gate** — if a player serves before the WebRTC channel is open, the
  first handoff packet is dropped silently; a "wait for peers" gate is wanted.
- **Server-side trajectory validation**, **TURN relay** (symmetric-NAT peers),
  and **usernames in the roster** remain in [`../planning/roadmap.md`](../planning/roadmap.md).
- **3D view** — implement `View3D : Node3D` against `IGameView`, mapping the same
  `(x, y)` data to `Vector3`. Simulation and networking are untouched.
