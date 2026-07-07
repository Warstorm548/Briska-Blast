# Match lifecycle — the lobby → game handoff

The client's session lifecycle is owned by one orchestrator: the **`MatchFlow`**
autoload (`client/src/core/MatchFlow.cs`). It replaced the original quick-step
handoff where UI scenes created and reparented the live network themselves —
the start choreography existed twice (lobby start + process-death rejoin) and
the game scene was entered before the WebRTC mesh existed, so an early serve
was silently dropped.

This is **Stage A** of a three-stage rework agreed 2026-07 (this doc grows with
each stage):

- **Stage A (game 0.23.0, shipped here)** — client `MatchFlow` state machine +
  the `Preparing` connecting phase.
- **Stage B (planned)** — server ready-barrier: a `client_ready` frame after
  mesh-up, the server flips the session `starting → active` and broadcasts
  `match_started` when all members are ready (or a ~20s grace elapses), plus a
  lobby poll fallback so a client that misses `start_signaling` recovers.
- **Stage C (planned)** — pause/resume on a mid-match rejoin: the server
  broadcasts `match_paused` while a process-death rejoiner re-meshes (everyone
  freezes behind the same `PreparingPanel` as an overlay) and `match_resumed`
  when the rejoiner readies up or a ~25s valve fires.

## The state machine

```text
            EnterLobby                start_signaling            mesh complete
   Idle ────────────────▶ InLobby ────────────────▶ Preparing ────────────────▶ InMatch
    ▲                                                   ▲                          │
    │                     BeginRejoin (process-death    │                          │ GameOver
    │                     rejoin; Identified carries    │                          ▼
    │                     the frozen seat_order) ───────┘                      PostMatch
    │                                                                              │
    └───────────────── LeaveSession / EndMatchTo / any failure ────────────────────┘
```

One private `TransitionTo(to, why)` gate enforces the table and logs every
change under the **`match.flow`** log category; anything illegal (a duplicate
`start_signaling`, a `SessionEnded` after the end screen is up, a `GameOver`
while still in the lobby) is logged and ignored. There are no per-scene
`_leaving`-style one-shot flags anymore — the state *is* the guard.

- **`Idle`** — no session. Main menu / host setup / join screens.
- **`InLobby`** — signaling socket open, roster live, waiting for Start.
- **`Preparing`** — the connecting phase (below). Entered from the lobby on
  `start_signaling`, or from `BeginRejoin` for a process-death rejoin — **both
  paths converge here** and share the same `StartMesh` bring-up.
- **`InMatch`** — `GameScene` runs. Entered only once the mesh is up.
- **`PostMatch`** — the server's `GameOver` arrived; the end-game screen owns
  navigation and the trailing `SessionEnded` is expected (ignored).

## Ownership rules

- **MatchFlow owns the live net.** The `SignalingClient` is created fresh per
  session and the `WebRtcMeshTransport` per match, both as children of the
  autoload — they survive every `ChangeSceneToFile` without the old
  `SessionContext.AdoptNet` reparenting trick (deleted). Scenes read them via
  `MatchFlow.Signaling` / `MatchFlow.Transport` and never create or free them.
- **Event ownership split.** MatchFlow is the *sole* subscriber of
  lifecycle-mutating signaling events — `Identified`, `PeerJoined`, `PeerLeft`,
  `HostChanged` (the only code that mutates the `SessionContext` roster),
  `StartSignaling`, `SessionEnded`, `Kicked`, `Closed`, `GameOver`. Views
  subscribe directly (via `MatchFlow.Signaling`) only to **pure-UI** events:
  `ChatMessage`, `Reconnecting`/`Reconnected`, the `Host/PeerReconnecting`
  overlays, and `ScoreUpdate` + the offer/answer/ICE relays consumed by
  `NetGameController`/the transport (net glue, not lifecycle).
- **MatchFlow's typed surface for views:** `StateChanged`, `RosterChanged`
  (re-render the lobby), `PreparingProgress` (+ the pull-on-entry
  `PreparingStatus`), `MatchEnded` (the GameOver relay `GameScene` consumes).
- **`SessionContext` is pure data**: identity, the one `ServerApi`, and session
  fields (code/mode/rules/roster/seat order/usernames). `MatchFlow.IsRejoin`
  replaced `RejoinInProgress`.
- A WS **`Reconnecting` is never a lifecycle event** — the socket re-dials for
  ~30s on its own; only a terminal `Closed` fails the flow.

## The Preparing phase

`Preparing` is what used to not exist: the gap between "the match started" and
"every peer connection is actually usable".

- The **`PreparingScreen`** (`ui/menus/PreparingScreen.tscn`) shows a reusable
  **`PreparingPanel`** ("Connecting to players (n/m)…", session code + copy,
  Cancel). The panel is a separate scene so Stage C can re-instantiate it as an
  in-game pause overlay.
- The transport has no aggregate "all connected" signal, so MatchFlow counts its
  per-peer `PeerConnected` events against the expected roster (start roster
  minus self), folding in `PeerFailed`/`PeerDisconnected`; a `PeerLeft` during
  Preparing removes that member from the expected set so a ghost can't hang the
  phase. Completion = expected ⊆ connected → `InMatch` → `GameScene`.
- **Deadline 30s** (`PrepareTimeoutMsec`; sized to the WS reconnect window /
  server promotion grace, with room for TURN ICE), plus an early fail once every
  expected peer resolved and any failed. Failure lands on the main menu with a
  read-once reason (`TakeFlowError`).
- Because `GameScene._Ready` (which builds `GameState` and spawns the host's
  serve ball) only runs after completion, the host **cannot serve into an
  unopened channel** anymore — the client-side half of the serve gate. Stage B
  makes it server-authoritative.
- Chat has no subscriber during Preparing; lines broadcast in that window are
  simply not shown to that client (deliberate — not worth buffering).

## The one teardown

Every exit funnels through one private `Teardown`:

- `LeaveSession(sendLeaveFrame)` → main menu. `sendLeaveFrame: true` sends the
  voluntary `leave` (frees the lobby slot immediately — the joiner's "Leave
  Session" button); `false` is a plain close, which the server treats as a
  **transient drop** (slot held for the reconnect grace, promotion timer for a
  host) — exactly what the mid-match "Exit to main menu" relies on.
- `EndMatchTo(scenePath)` → same teardown, different destination (end-game
  "Host Game", the lobby host's "Return to Setup").
- `QuitGame()` → same teardown, then quit.
- All failures (`FailFlow`) set `LastFlowError` and run `LeaveSession(false)`;
  the main menu shows the reason once.

## Known limits (until Stages B/C land)

- `start_signaling` is still a best-effort broadcast; a client that misses it
  (WS mid-reconnect at Start) stays in the lobby. Stage B's poll fallback fixes
  this.
- The server still never sets `SessionStatus::Active`; matches live in
  `Starting`. Stage B's ready-barrier makes `Active` real.
- A mid-match rejoiner re-meshes while the game keeps running; balls sent at
  its edge before the mesh heals bounce off the temporary wall. Stage C pauses
  the match for the re-mesh instead.
