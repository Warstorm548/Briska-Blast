# Match lifecycle — the lobby → game handoff

The client's session lifecycle is owned by one orchestrator: the **`MatchFlow`**
autoload (`client/src/core/MatchFlow.cs`). It replaced the original quick-step
handoff where UI scenes created and reparented the live network themselves —
the start choreography existed twice (lobby start + process-death rejoin) and
the game scene was entered before the WebRTC mesh existed, so an early serve
was silently dropped.

All three stages of the rework agreed 2026-07 have shipped:

- **Stage A (game 0.23.0)** — client `MatchFlow` state machine +
  the `Preparing` connecting phase.
- **Stage B (server 0.23.0 / game 0.24.0)** — server
  ready-barrier: a `client_ready` frame after mesh-up, the server flips the
  session `starting → active` and broadcasts `match_started` when all members
  are ready (or a 20s grace valve fires), plus a lobby poll fallback so a
  client that misses `start_signaling` recovers.
- **Stage C (server 0.24.0 / game 0.25.0)** — pause/resume on a mid-match
  rejoin: the server broadcasts `match_paused` while a process-death rejoiner
  re-meshes (everyone freezes behind the same `PreparingPanel` as an overlay)
  and `match_resumed` when the rejoiner readies up, drops again, or a 25s
  valve fires.

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
  `StartSignaling`, `SessionEnded`, `Kicked`, `Closed`, `GameOver`,
  `MatchStarted`, `MatchPaused`/`MatchResumed` (plus `Reconnected`, for the
  ready re-send below — views may also subscribe to that one). Views
  subscribe directly (via `MatchFlow.Signaling`) only to **pure-UI** events:
  `ChatMessage`, `Reconnecting`/`Reconnected`, the `Host/PeerReconnecting`
  overlays, and `ScoreUpdate` + the offer/answer/ICE relays consumed by
  `NetGameController`/the transport (net glue, not lifecycle).
- **MatchFlow's typed surface for views:** `StateChanged`, `RosterChanged`
  (re-render the lobby), `PreparingProgress` (+ the pull-on-entry
  `PreparingStatus`), `MatchEnded` (the GameOver relay `GameScene` consumes),
  and the pause relays `MatchPausedFor`/`MatchResumedIn` (InMatch only).
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
  phase. Mesh complete = expected ⊆ connected.
- **The ready barrier (Stage B).** Mesh completion doesn't enter the match —
  it sends `client_ready` and holds ("Waiting for other players…"). Server
  side, the frozen start roster is seeded on the signaling room at `/start`
  (beside the win target, in-memory like scores); when every seated player's
  ready is in — or a **20s grace valve** (`READY_GRACE_SECS`, a plain spawned
  timer; the `match_started` latch is the single-winner between the two) fires
  first — the server broadcasts **`match_started`** and flips the session
  `starting → active` via a Lua CAS. `match_started` is the **only door into
  `InMatch`**; a ready arriving after resolution (straggler, poll recovery,
  Stage C rejoiner) gets a direct reply, so everything converges on "send
  ready, wait for match_started". A WS blip mid-wait re-sends the ready on
  reconnect. Since `GameScene._Ready` (which spawns the host's serve ball) now
  runs only after the barrier, the serve gate is **server-authoritative**.
- **Deadline 30s** (`PrepareTimeoutMsec`; sized to the WS reconnect window /
  server promotion grace, with room for TURN ICE — and above the server's 20s
  valve, so the barrier wait itself can only time out if the server is
  unreachable), plus an early fail once every expected peer resolved and any
  failed. Failure lands on the main menu with a read-once reason
  (`TakeFlowError`).
- **Lobby safety-net poll.** `start_signaling` is a best-effort broadcast; a
  client whose WS is mid-reconnect at Start misses it. While `InLobby`,
  MatchFlow polls `GET /session/:code` every **7s** (a 4-player lobby behind
  one NAT stays under the shared 60/min per-IP session limiter). If the
  session left `waiting` with no `start_signaling` received, it recovers
  through the rejoin convergence: adopt the poll's rules, swap the lobby
  socket for a fresh `Identify` (whose `Identified` reply carries the frozen
  `seat_order` + the match's cached TURN credentials), and mesh behind the
  connecting screen. `IsRejoin` (no-serve semantics) is set only when the
  session is already `active` — recovering into `starting` means nobody has
  served yet, so a recovered **host** still serves normally. The
  first-identify mesh bring-up keys on "in `Preparing` with no transport yet"
  (the normal start path builds its transport synchronously in
  `start_signaling`, so it never lands there).
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

## Pause-on-rejoin (Stage C)

A process-death rejoin used to re-mesh while the game kept running — balls
sent at the rejoiner's edge bounced off a temporary wall. Now the match
freezes for the re-mesh:

- **Only a true rejoin pauses.** The `identify` frame carries `rejoin: true`
  exactly on the client's rejoin paths (`BeginRejoin`, the lobby poll's
  recovery into an `active` match). It is **cleared the moment the client
  enters the match**, so a later transient WS auto-reconnect — same process,
  mesh intact — re-identifies as a normal member; and the *initial* mid-game
  drop never pauses anything (peers just see the reconnect overlay, as
  before).
- **Server side**, a rejoin identify into a started match (the barrier's
  `match_started` latch is the "started" test) places a **pause hold** on the
  room and broadcasts `match_paused { player_id, username,
  resume_timeout_secs: 25 }`. Holds are a set, so overlapping rejoiners stack
  and the match resumes only when the **last** hold clears. Three racers
  release a hold — the rejoiner's `client_ready` (released just before its
  direct `match_started` reply, so the room's countdown is already running
  when the rejoiner lands), the rejoiner disconnecting again, and a **25s
  valve** (`PAUSE_VALVE_SECS`, under the rejoiner's own 30s Preparing
  deadline) — all funneling through one `resume_if_cleared`, whose remove-wins
  semantics make the `match_resumed { countdown_secs: 3 }` broadcast
  single-shot.
- **Client side**, MatchFlow relays these as typed `MatchPausedFor(name)` /
  `MatchResumedIn(secs)` (fired only while `InMatch`). `GameScene` freezes the
  whole physics tick behind a `_flowPaused` latch (mirroring `_gameOver`) and
  shows the reused `PreparingPanel` as an overlay — "Waiting for {name} to
  reconnect…", Cancel hidden. On resume it paints a 3-2-1 countdown, then
  every screen unfreezes together. If the valve resumed before the rejoiner's
  mesh healed, its edge is simply walled until the re-mesh completes (the
  pre-Stage-C behavior, now bounded to that window).

## Known limits

- A server restart mid-barrier or mid-pause loses the in-memory ready/pause
  state (like scores): clients fail back to the menu on their 30s Preparing
  deadline, or unfreeze late via their own overlays. Acceptable — a restart
  kills the live match anyway.
- If the **host** misses `start_signaling` *and* the 20s valve starts the
  match before its poll recovery lands, the host recovers with rejoin
  (no-serve) semantics into a match nobody has served — a ball-less match.
  Requires the host's WS to blip exactly across Start
  plus a >20s recovery; revisit if it ever shows up in the field.
