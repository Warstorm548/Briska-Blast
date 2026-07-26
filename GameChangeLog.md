# Game Changelog

All notable changes to the Briska Blast game are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> Renamed from `ClientChangeLog.md` on 2026-05-23 to match the
> `game-v*` release-tag namespace and avoid confusion with launcher /
> server changelogs. Content prior to the rename is preserved below.

---

## [0.29.0] — 2026-07-26

**Moderator chat lines.** A moderator can now speak into a session from the admin
Chat-Mod panel (server 0.31.0). Their line renders distinctly in the lobby chat —
`[MOD] <name>: <text>` in blue — so staff never read as another player.

The name shown is whatever the moderator chose to appear as: either their real
display name or a generic `Mod`. The client is deliberately not told which, and
never learns who is behind an anonymous post; that attribution lives server-side
in the moderation record.

Styling goes through `PushColor`/`AddText`/`Pop`, never `AppendText`, preserving
the existing guarantee that server- and user-supplied strings are never parsed as
BBCode and so cannot inject tags.

A moderator line carries no sender id, so it bypasses the roster entirely —
feeding an empty id to `SessionContext` would have registered a phantom entry and
then labelled the line from it.

`SignalingClient.ChatMessage` now carries a `ChatLine` record rather than three
positional strings; chat had grown a sender kind and would have kept growing.

**Censoring needs nothing here.** Blacklisted words arrive already masked — the
server censors before broadcasting, so the raw word never reaches a client and
there is nothing to filter client-side.

Chat still has no in-match subscriber; moderator lines appear in the lobby only.

> **Deploy:** no `min_game_version` change required — this release only adds
> styling. Older clients receive a moderator line and render it as an ordinary
> chat message, which is correct if unstyled. Bump `min_game_version` to
> **0.29.0** only if you want the `[MOD]` treatment guaranteed for everyone.

---

## [0.28.1] — 2026-07-19

Diagnostic instrumentation for a TURN-relay-only ball-handoff bug: on the relayed
side the ball enters a player's screen displaced **inward from the top edge**
(delayed, and further into the field than it should be), while direct-WebRTC peers
are unaffected and the return direction (relayed → WebRTC) is normal. No gameplay
change — a temporary logging build to confirm the cause from field logs before the
fix. The one-directional symptom points at a **biased receiver server-clock offset**
amplified by the handoff transit fast-forward (`pos += vel * transit`), not at
macOS or the TURN transport itself.

> **Deploy:** no `min_game_version` change — arena geometry is unchanged from 0.28.0,
> so 0.28.0 and 0.28.1 clients interoperate.

### Added

- **Handoff transit logging** (`src/game/net/NetGameController.cs`): each incoming
  ball logs the raw (pre-clamp) vs used fast-forward transit and the resulting inward
  push as a fraction of arena height (`Log.Info`, category `game.handoff`). Handoff
  IN/OUT lines resolve the peer's display name (via `SessionContext.DisplayNameFor`)
  so a capture reads `peer=<name>` rather than an opaque player_id.
- **Clock-sync logging** (`src/net/SignalingClient.cs`): each `time_sync` reply logs
  the round-trip, this probe's raw sample, its deviation from the running estimate,
  and the smoothed server-clock offset (`Log.Info`, category `net.clock`) — a distant
  client's samples jump around, exposing a biased offset.
- **`ServerClock.OffsetMs`** (`src/net/ServerClock.cs`): read-only accessor exposing
  the offset estimate for the above.

All logging is `Info`-level so it surfaces on every channel, and tagged
`TEMP diagnostic` for removal once the cause is confirmed.

## [0.28.0] — 2026-07-18

A styling pass on the hotbar shipped in 0.27.0: new slot art, slots rendered at the
new sprite's native size, and a metallic backing in place of the placeholder gray.
No behaviour change — the bar still holds 5 keybound slots and no items exist yet.

> **Deploy:** bump `min_game_version` to **0.28.0** before rolling out. Slots shrink
> 105px → 96px, which changes the height of the bar and therefore of the play field.
> Handoff speed and entry position cross the wire normalized by arena height
> (`NetGameController`), so a 0.27.x client and a 0.28.0 client in the same match
> would disagree by the 9px difference.

### Changed

- **Slot art** (`src/assets/sprites/ActionBarArea/ItemSlotV3.png`, replaces
  `ItemSlotV2.png`): a **96×96** frame — 6px of near-black `#191919` on all four sides
  around an **84×84** interior that fades to a teal centre. The border is a touch
  softer than V2's pure `#000000`. Note the interior is `96 − 2×6`, not `96 − 6`: the
  frame is subtracted from **both** sides, so a 6px border on a 96px sprite can only
  leave 84px of icon space. A 90×90 interior would need either a 3px border or a
  102×102 sprite.
- **Slots render at native size** (`HotbarView.SlotSizeHFrac`): `105/1440` → `96/1440`
  of viewport height, keeping the convention that on-screen size matches the sprite's
  native resolution at the 2560×1440 design size, so the art never resamples. The
  strip is one slot tall as before — 7.29% → **6.67%** of screen height — and the
  5-slot row spans 18.75% of the width (was 20.5%).
- **The play field grew by the 9px the bar gave up** (`src/game/GameScene.cs`,
  unchanged): the arena is still derived as viewport minus bar height, so the shorter
  bar widens the field for free. Absolute paddle speed, ball radius and paddle size
  rise ~0.7% with the taller field; they stay proportional to it, and every client
  changes identically.
- **Action-bar backing** (`HotbarView.StripColor`): the placeholder light gray
  `#C7C7C7` → a flat **`#6B7078`**, a slightly blue-tinted mid-dark gray. The cool
  tint is what reads as brushed metal rather than flat gray, and it picks up the teal
  in the slot art. Flat fill for now; a gradient would sell the metal harder if this
  is revisited.
- **`IconSizeFrac` removed** (`src/ui/HotbarView.cs`): it was never read. The layout
  insets the icon via anchor offsets from `IconInsetFrac` alone, and for a square
  sprite the size is implied (`1 − 2 × inset`). Correcting 0.27.0's note below:
  resizing the bar is **two** constants — `SlotSizeHFrac` and `IconInsetFrac` — plus
  a matching sprite.

---

## [0.27.0] — 2026-07-18

A **hotbar / action bar** below the play field — the container for player-actionable
items. No items exist yet; this ships the bar, the slot model, the keybinds, and the
screen space they live in, so item content can drop straight in later.

> **Deploy:** bump `min_game_version` to **0.27.0** before rolling out. The play
> field is now shorter than the viewport, and handoff speed / entry position cross
> the wire normalized by arena height (`NetGameController`), so a 0.26.x client and
> a 0.27.0 client in the same match would disagree by the height of the bar.

### Added

- **Hotbar** (`src/ui/HotbarView.cs`, new): a full-width light-gray strip pinned to
  the bottom of the screen holding **5 flush item slots** centered as a block — no
  gap between adjacent slots, with the backing visible to the left and right of the
  row. Sits on its own `CanvasLayer` (50), below the reconnect overlay (100) and the
  pause / end-game menus (200), so both still cover it.
- **Slot sizing is viewport-relative**: a slot is `105/1440` of viewport **height** —
  the sprite's native 105×105 at the 2560×1440 design size — so the bar occupies the
  same share of the screen on any display or aspect ratio, matching the `HFrac`
  convention already used for the paddle, ball and corner barriers. The strip is
  exactly one slot tall (7.29% of screen height; the row spans 20.5% of the width).
- **Item icons** render in the slot sprite's inner square: 93×93 inset 6px inside the
  105×105 frame, held as ratios (`6/105`, `93/105`) so both survive rescaling and the
  placeholder art being redrawn. Slots show only the frame and, when filled, the
  icon — no slot numbers.
- **Slot art** (`src/assets/sprites/ActionBarArea/ItemSlotV2.png`): the placeholder
  frame regenerated at 2.5× the original 42×42 (→ 105×105) with the black border
  **capped at 6px** instead of scaled, so the usable icon area grew **2.58×** (36 → 93)
  — more than the slot itself. Colours are reproduced exactly from the original: its
  gradient depends only on Chebyshev distance from the centre (square rings, not a
  radial circle — the inner corner and edge midpoint carry the same colour, and each
  ring varied by at most 1/255), so the 18-entry ring palette was resampled to the new
  size rather than redrawn. Centre `(89,131,146)` and inner edge `(28,29,30)` are
  byte-identical to it. The superseded `ItemSlotV1.png` was removed; it remains in
  history if ever needed.

  Resizing again is three constants — `HotbarView.SlotSizeHFrac` plus the
  `IconInsetFrac` / `IconSizeFrac` pair — and a matching sprite; nothing else in the
  bar or the play field carries a slot dimension.
- **Keys 1–5** (`hotbar_slot_1`…`hotbar_slot_5`, physical keycodes 49–53) each fire
  their own slot. Bound as named input actions rather than hardcoded keycodes so they
  can be rebound later. Polled in `GameScene._PhysicsProcess` below the `_gameOver`
  and flow-pause returns, so keys are dead behind the end screen and during a rejoin
  freeze, and suppressed with the pause menu open like the paddle and serve. Live
  during the pre-serve wait.
- **Press feedback**: a fired slot flashes white and fades over 120ms — temporary, so
  the keybinds are observable before items exist, and the natural home for
  "item used" feedback once they do.
- **`ItemSlot` / `Hotbar` model** (`src/game/GameState.cs`): a fixed row of 5 slots
  on `GameState` alongside `Paddle`, each holding an optional icon and a count. Local
  only, never networked. Per-item **maximum stack sizes** will live on a future item
  lookup table, not on the slot — a slot shouldn't decide its own cap.
- **`AssetCategory.Ui`** (`src/core/SpriteRegistry.cs`): a fourth category for screen
  furniture. The existing three all answer "who moves this thing in the arena", which
  no UI sprite has an answer to. `SystemSpawns()` is unaffected. New
  `AssetId.ItemSlot = 7` → `src/assets/sprites/ActionBarArea/ItemSlotV2.png`.

### Changed

- **The play field no longer fills the viewport** (`src/game/GameScene.cs`): `_Ready`
  now derives the arena as the viewport minus the bar's height, so the field sits
  entirely **above** the hotbar and the ball is still seen crossing the bottom goal
  line to score. Resolving it as one local means the paddle line, the corner-barrier
  colliders, the ball radius and `ArenaWidth`/`ArenaHeight` all follow automatically —
  in particular the colliders and their sprites keep being built from the same
  numbers, preserving `CornerBarrier`'s collider-can't-drift-from-art invariant.
  Absolute paddle speed, ball radius and paddle size shrink ~2.9% with the shorter
  field; they stay proportional to it, and every client shrinks identically.

---

## [0.26.2] — 2026-07-15

Follow-up tweak to the **Credits** screen spacing.

### Changed

- **Credits header underline spacing** (`src/ui/menus/CreditsMenu.tscn`): the
  underline (`HSeparator`) beneath the **Name / Username** column headers is
  pulled back up tight to the header text (its original 10px gap) by grouping
  the header row + underline into their own `HeaderSection` `VBoxContainer`;
  the wider gap now falls *below* the underline, before the first section. All
  other spacing is unchanged. Layout-only.

---

## [0.26.1] — 2026-07-15

Visual polish for the **Credits** screen — a clearer typographic hierarchy.

### Changed

- **Credits page styling** (`src/ui/menus/CreditsMenu.tscn`): the page title,
  the **Name / Username** column headers, and every role/section heading now
  render in the game's **gold** (`Color(1, 0.85, 0.4, 1)` — the same gold as the
  end-game leaderboard title); entry text (names, usernames, fun phrase) stays
  white. Row spacing is **doubled** (10 → 20) and each section (heading + its
  rows) is grouped into its own `VBoxContainer` so sections are separated by a
  larger gap (44), reading as coherent blocks instead of an evenly-spaced list.
  Layout-only — no code or protocol changes.

---

## [0.26.0] — 2026-07-12

Adds a **Credits** screen and re-tunes the **Set Score** win condition to
default **100**, range **50–200**.

### Added

- **Credits page** (`src/ui/menus/CreditsMenu.tscn` + `.cs`): the previously
  disabled main-menu **Credits** button is now enabled and opens a credits
  screen on the default background — a two-column **Name / Username** table
  grouped by role (Lead Developer; Lead Architecture Adviser and Pre-Alpha
  Tester; Pre-Alpha Testers) plus a **Fun Phrases** section — with a Return to
  Main Menu button. Mirrors the `SettingsMenu` sub-page pattern.

### Changed

- **Set Score range** (`Dto.cs` mirror of `shared` 0.6.0): the host's Advanced
  Settings → Match Rules score now defaults to **100** and is clamped
  **50–200** (was 11 / 10–50). Requires server **0.25.0**.

---

## [0.25.0] — 2026-07-07

Adopts the server's **pause-on-rejoin** — Stage C, the final stage of the
handoff rework (`docs/architecture/match-lifecycle.md`). Requires server
**0.24.0** (`min_game_version` is bumped alongside).

### Added

- **Rejoin identifies declare themselves**: the `identify` frame carries
  `rejoin: true` on the process-death rejoin paths (`BeginRejoin`, the lobby
  poll's recovery into an `active` match) so the server pauses the live match
  while this client re-meshes. The flag is cleared the moment the client
  enters the match, so a later transient WS blip re-identifies as a normal
  member and can never pause anyone.
- **Match freeze + pause overlay**: on `match_paused`, `GameScene` freezes the
  whole tick (input, spawns, sim, handoffs — mirroring the `_gameOver` latch)
  behind the reused `PreparingPanel` as an overlay — "Waiting for {name} to
  reconnect…", no Cancel. A second rejoiner just updates the name; the match
  stays frozen until the **last** hold clears server-side.
- **3-2-1 resume countdown**: on `match_resumed`, the overlay counts the
  server's `countdown_secs` down and every screen unfreezes together. The
  rejoiner itself is still on the connecting screen through all of this — its
  own go-signal remains `match_started` (it typically lands mid-countdown).

## [0.24.0] — 2026-07-07

Adopts the server's **ready barrier** and adds a **lobby safety-net poll** —
Stage B of the three-stage handoff rework
(`docs/architecture/match-lifecycle.md`). Requires server **0.23.0**
(`min_game_version` is bumped alongside; the protocol gained
`client_ready`/`match_started`).

### Added

- **Ready-barrier hold in `Preparing`**: mesh completion no longer enters the
  match directly — the client sends `client_ready` and shows "Waiting for other
  players…" until the server's `match_started` (broadcast when everyone is
  ready, or its 20s valve fires — always under the 30s Preparing deadline).
  `match_started` is now the **only door into `InMatch`**, making the serve
  gate server-authoritative: nobody serves until every player's mesh is up. A
  WS blip during the wait re-sends the ready on reconnect (the server answers
  a duplicate directly).
- **Lobby safety-net poll**: `start_signaling` is a best-effort broadcast — a
  client whose WS is mid-reconnect at Start used to be stranded in the lobby
  forever. While `InLobby`, `MatchFlow` now polls `GET /session/:code` every
  7s (a full 4-player lobby behind one NAT stays under the server's 60/min
  per-IP limiter); if the session left `waiting` with no `start_signaling`
  received, it recovers through the rejoin sequence — fresh identify (frozen
  `seat_order` + the match's cached TURN credentials), mesh behind the
  connecting screen — with rejoin (no-serve) semantics only when the match is
  already `active`.

### Changed

- `MatchFlow`'s first-identify mesh bring-up now keys on "entered `Preparing`
  without a transport" rather than the rejoin flag, so the poll recovery and
  the process-death rejoin share the same convergence path; the signaling-
  socket close is extracted into one `CloseSignaling` shared by the teardown
  and the recovery.

## [0.23.0] — 2026-07-07

Reworks the **lobby → game handoff** around a single lifecycle orchestrator —
the long-term replacement for the quick-step transition that entered the game
before the WebRTC mesh existed. Stage A of the three-stage handoff rework (next:
a server ready-barrier, then pause-on-rejoin); see
`docs/architecture/match-lifecycle.md`.

### Added

- **`MatchFlow` autoload** (`src/core/MatchFlow.cs`) — a lifecycle state machine
  (`Idle → InLobby → Preparing → InMatch → PostMatch`) with a single legal-
  transition gate (logged under the new `match.flow` category; duplicate or late
  frames are ignored there instead of by per-scene one-shot flags). It owns the
  `SignalingClient` + `WebRtcMeshTransport` as its own children for their whole
  life, is the sole subscriber of lifecycle-mutating signaling events, and has
  **one** start sequence, **one** rejoin sequence (both converge in `Preparing`),
  and **one** teardown (`LeaveSession` / `EndMatchTo` / `QuitGame`) that
  preserves the deliberate leave-frame vs transient-drop distinction.
- **"Connecting to players…" phase** — a new `PreparingScreen` (wrapping a
  reusable `PreparingPanel`, later reused as the pause-on-rejoin overlay) shows
  while per-peer data channels open, counted against the start roster, with a
  **30s deadline**; the phase also short-circuits before the deadline once
  every expected peer has either connected or definitively failed (with at
  least one failure) — a single failed peer doesn't abort while others are
  still negotiating. The
  game scene is only constructed once the mesh is up, so the host can no longer
  serve into an unopened channel (the long-deferred **serve gate**, client-side
  half).
- **Main-menu failure line** — every abnormal session end (connect timeout,
  kick, session closed, rejoin refused) lands on the main menu with a read-once
  reason from `MatchFlow.TakeFlowError()`.

### Changed

- **`SessionLobby` / `JoinMenu` / `GameScene` are thin views now.** The start
  choreography that existed twice (the lobby's `OnStartSignaling` and the
  ~80%-duplicate rejoin path in `JoinMenu`) moved into MatchFlow once; the
  scenes keep only pure-UI subscriptions (chat, reconnect overlays, score
  paints) and route every exit through the one teardown.
- **`SessionContext` is pure data again** — `AdoptNet`/`TeardownNet` (the
  node-reparenting trick that carried the live net across scene changes), the
  `Signaling`/`Transport` properties, and the `RejoinInProgress` flag are gone;
  MatchFlow replaces them all (`MatchFlow.IsRejoin` covers the rejoin flavor).

## [0.22.0] — 2026-07-06

Reshapes the corner barriers from an **L into a right triangle** (new
`Cornerbarrier.png` art) and gives players a one-click way to **copy the session
code** from the lobby and the in-game pause menu.

### Changed

- **Corner-barrier collider is now a triangle.** `CornerBarrier` emits one right
  triangle per corner (`AppendTriangles` → `List<BarrierTri>`) instead of two
  axis-aligned `Rect2` bars, mapped through the exact same pivot + 90°-rotation
  transform the view uses, so art and collider still can't drift. The collision
  surface is inset **1px into the solid** (uniform incenter scale) so the ball
  overlaps the art slightly before bouncing.
- **`GameSimulation.ResolveBarriers` is now circle-vs-triangle.** Closest-point-on-
  triangle (Ericson) + true-normal reflection `v − 2(v·n)n`, applied only when the
  ball moves into the surface. On the diagonal hypotenuse this gives an **angled
  deflection** that turns shots away from the goal corner, replacing the old flat
  X/Y-axis bounce. A deeply-penetrated centre exits along its shallowest edge.
  Shared geometry helpers (`PointInside`, `ClosestPoint`, `NearestEdgeExit`,
  `Overlaps`) live on `CornerBarrier`; the splitter spawn-guard reuses `Overlaps`.

### Added

- **Copy-session-code button** beside the code in the **lobby**
  (`SessionLobby.tscn`) and the **ESC/pause menu** (`PauseMenu.tscn`) — an
  icon-only, transparent `TextureButton` using a new generated clipboard glyph
  (`src/assets/sprites/ui/CopyIcon.png`). Copies to the OS clipboard via
  `DisplayServer.ClipboardSet` and flashes a brief **"Copied!"** confirmation. No
  button on the play field.

## [0.21.0] — 2026-07-04

Consumes the server-minted **Cloudflare TURN credentials** (server 0.22.0), so
peers behind symmetric NATs — the confirmed Win↔Mac field failure — connect via
a relay instead of ICE dying in `Connecting` and every handoff dropping.

### Added

- **`IceServerDto`** (`src/net/Dto.cs`) mirroring the server's `IceServer`
  (`turn.rs`), parsed tolerantly from the new `ice_servers` field on the
  `start_signaling` and `identified` frames (absent/malformed ⇒ empty, so old
  servers keep working).
- **`WebRtcMeshTransport.SetIceServers`**: adopts the server-minted STUN+TURN
  list for all subsequently created peer connections; an empty list keeps the
  built-in Google-STUN fallback (old server / TURN unconfigured / mint failed).
  Logs `using N server-provided ice servers (turn=yes|no)` — the field-log tell
  that relay candidates are possible.
- Wired at both mesh bring-up sites: `SessionLobby.OnStartSignaling` (normal
  match start) and `JoinMenu.OnRejoinIdentified` (process-death rejoin, whose
  fresh process gets its own credential set in the `identified` frame), always
  before `Connect`.

## [0.20.0] — 2026-07-03

Adds a **permanent, structured logging system** with a persisted per-run log file,
and instruments the WebRTC/handoff path so peer-connection problems (the "balls
don't cross portals" class of bug) are finally visible in a file users can send.

### Added

- **Central `Log` autoload** (`src/core/Log.cs`): leveled (Trace/Debug/Info/Warn/
  Error), category-tagged (`net.webrtc`, `net.signaling`, `game.handoff`, `session`,
  `single-instance`, …), timestamped structured lines. Every line is mirrored to the
  Godot console **and** written to a per-run file. Debug on the `dev` channel, Info
  on ea/stable.
- **Per-run log file** in the launcher-known per-user data dir, in a **per-channel
  folder** (`logdev` / `logea` / `logstable`, resolved by the new shared
  `src/core/Paths.cs`). One file per game launch, named
  `briskablast_<channel>_<timestamp>.log`; the newest 20 runs are kept, older ones
  pruned on open. A rejected duplicate instance writes no file, and AutoFlush keeps
  the tail on disk through a hang/crash. A startup banner records version, channel,
  platform, renderer, resolution, and the resolved log path.
- **WebRTC instrumentation** (`WebRtcMeshTransport`): logs ICE connection-state
  transitions, gathering state, ICE **candidate types** (host/srflx/relay — the NAT
  story), data-channel open/close, and offerer/answerer role — so a stalled or failed
  peer link is diagnosable from the log alone.
- **Handoff instrumentation** (`NetGameController`): logs each ball handoff in/out
  and — via a new `IPeerTransport.Send` success bool — a **Warn when a ball is handed
  to a not-yet-open channel** (the previously-silent drop that looks like "no balls
  crossing").

### Changed

- `SingleInstance` resolves the data dir through the shared `Paths` helper (the
  per-platform logic moved there unchanged), and the net / session / single-instance
  `GD.Print`/`PushWarning` calls were migrated onto `Log`.

## [0.19.0] — 2026-07-01

Adds **corner barriers**: a solid L-shaped obstacle in all four corners of every
player's screen that turns balls away from the goal corners and stops corner-cutting.

### Added

- **Corner barriers.** The `Cornerbarrier` sprite (drawn for the bottom-left corner) is
  placed in all four corners of every screen — the same sprite rotated in 90° steps
  about its bottom-left pixel, that pixel pinned to the screen corner, so each L opens
  toward the arena centre. Sized as a fraction of arena height (like the paddle / ball /
  goal-gap tuning), so every resolution gets the same proportion.
- **Barrier collision.** Balls (master and split) bounce off the barriers. Each L is
  modelled as its two solid bars (arm + foot) — 8 axis-aligned rects total — resolved
  in the sim before the goal/edge checks, so a ball entering a bottom goal corner is
  turned away instead of sneaking past the paddle. Reflection reuses the wall-bounce
  math; the transparent inner region of the L stays open.
- **`CornerBarrier` layout helper** (`game/CornerBarrier.cs`), the single source of
  truth for barrier geometry — the simulation derives its collision rects and the view
  places its sprites from the same corner/rotation/scale table, so the collider can
  never drift from the art.
- **`AssetCategory.SystemControlled`** for the barrier: game-owned but static (no spawn
  cadence), so it stays out of the host's Random-Spawns frequency UI. Registered as
  `AssetId.CornerBarrier` in the sprite registry.

### Changed

- Random splitter spawns now avoid the corner barriers, so a splitter can't appear
  unreachable inside one.

---

## [0.18.0] — 2026-06-29

Adds the **ball-splitter** mechanic: a system-spawned **BallSpliter** element that
splits the master ball into three double-value **BallBT** balls, plus a host-tuned
**Random Spawns** settings tab and a central **sprite registry**.

### Added

- **Asset/sprite registry** (`core/SpriteRegistry.cs`, autoload). A hand-curated
  lookup table mapping a stable, upward-counting `AssetId` to its `res://` path +
  category (`PlayerControlled` / `SystemHandled`), lazily caching the texture. Replaces
  the scattered texture constants in `View2D` — the single source of truth for sprite
  textures.
- **BallSpliter element + split mechanic.** A system spawn appears on each screen on a
  host-configured cadence (the cooldown doubles as its respawn timer). When the master
  ball touches it, it spawns **3 BallBT split balls** fanned 45° apart (centred away
  from the master's heading), inheriting the master's last-hitter; the master passes
  through unaffected. **Chain-splitting** (a split ball splitting again) is a host
  toggle, on by default.
- **BallBT split balls** are worth **2 points** and **vanish** at a goal (the master
  ball is re-served as before). They follow the same last-hitter possession rule and
  hand off between screens like any ball (the kind travels in the handoff packet).
- **Advanced "Random Spawns" settings tab** (above Match Rules): a splitter
  spawn-interval slider (5–60s, default 15) + a chain-split toggle. Sent to the server
  at host time and applied by every client (joiners included) so the cadence + rule are
  identical across the table.
- **Multi-ball support.** The sim now carries multiple concurrent balls: ball ids are
  seat-namespaced for global uniqueness, the serve no longer clears the ball list, and
  the loop always steps so split balls keep moving during a serve hold.

### Changed

- The score report now carries the ball's point value (1 master / 2 split) over the
  server-authoritative scoring channel.

---

## [0.17.0] — 2026-06-25

Isolates each release channel's self-contained **.NET runtime cache** and ships
**per-file integrity manifests** so the launcher can deep-verify and repair installs.

### Added

- **Per-channel runtime-cache isolation.** Each channel now exports with a distinct
  .NET assembly identity (`BriskaBlast` / `BriskaBlastEA` / `BriskaBlastDev`), so the
  game's self-contained runtime extracts to a channel-specific
  `data_<name>_<platform>` folder instead of a shared one — no cross-channel cache
  collision once more than one channel is installed. Driven entirely at build time in
  `release-client.yml`: it renames `project/assembly_name` plus the `<name>.sln` and
  `<name>.csproj` Godot derives from it (confirmed against Godot's source —
  `ExportPlugin.cs` / `path_utils.cpp`). The committed project files are unchanged and
  the export binary stays `BriskaBlast.*`, so the launcher's install/launch path is
  untouched. Matches the launcher's `Channel::cache_basename()`.
- **`files.json` integrity manifests.** Every release archive now carries a build-time
  manifest recording each shipped file's size + sha256, generated after signing and
  packaged inside the archive (beside the `.app` on macOS to preserve the ad-hoc
  signature). Consumed by the launcher's new deep Verify File Integrity.

### Changed

- The macOS "verify embedded C#" CI gate now asserts the per-channel
  `data_<name>_macos_<arch>/<name>.dll`, doubling as confirmation that the rename hit
  the folder + DLL in lockstep (a mismatch fails the build before any release publish).

## [0.16.0] — 2026-06-22

Adds the game's first **win condition** — "Set Score": first player to a
host-chosen target (10–50, default 11) wins — with an end-game leaderboard screen
and a new advanced-settings layout in Host Setup.

### Added

- **Win condition "Set Score".** The match ends the instant a player reaches the
  target. Configured in Host Setup's **Advanced → Match Rules** tab: a segment
  heading, a `Win Condition` label beside its dropdown, an inline score input
  (10–50) shown only for score-based kinds, and a live description that updates with
  the selection/value. Defaults to Set Score / 11, so a host who never opens the tab
  still sends a valid rule. Carried through host/join/poll and the `start_signaling`
  frame so every player applies the rule the server enforces.
- **End-game screen.** On the server's new `GameOver` frame the simulation freezes
  (no background ticking) and an overlay — styled like the pause menu, with the
  frozen game dimmed behind it — shows the winner, a **leaderboard of every player**
  in the session (0-point players included, winner highlighted), and **Return to
  Main Menu** / **Host Game** buttons.

### Changed

- The game scene ignores the post-win `SessionEnded` teardown while the end screen
  is up, so the leaderboard owns navigation (the server reuses `SessionEnded` for
  cleanup rather than a parallel path).
- Out-of-range host score input is refused by the server (`invalid_win_condition`);
  the Host Setup screen surfaces it as "Score must be 10–50."
editor too, so the second of two editor instances detected the first's banner,
flagged itself a duplicate, and quit before it could join — breaking the
documented two-editor-instance host/join test flow (`SessionContext`'s
`SelfRegisterAsync` self-provisions throwaway identities for exactly that).

### Fixed

- **`SingleInstance` now skips the guard in the editor** (`OS.HasFeature("editor")`),
  mirroring the editor escape hatches already in `SessionContext`. Two editor
  instances can host/join on one machine again. Exported release builds report
  `editor=false` and remain single-instanced for real users — the guard is
  unchanged for them.

---

## [0.15.0] — 2026-06-21

Adds **game single-instance** and **one-game-channel-at-a-time** enforcement via
the same socket rendezvous the launcher uses (**launcher v0.16.0**). A new
`SingleInstance` autoload binds an ephemeral `127.0.0.1:0` port, claims a single
shared `game_instance.json` (one file across all channels), and serves a
handshake banner so a second game — of any channel — detects the live one and
quits.

### Added

- **`SingleInstance` autoload** (`src/core/SingleInstance.cs`), registered above
  `SessionContext` so it runs first. On `_EnterTree` it binds `127.0.0.1:0`,
  atomically `create-new`s `game_instance.json` with the chosen port, and runs a
  background accept loop serving `BRISKA-BLAST\t1\tGAME\n`. A duplicate (a live
  GAME banner already answers) sets a static `IsDuplicate` flag and quits via a
  deferred `GetTree().Quit()`; `MainMenu` also checks `IsDuplicate` at the top of
  `_Ready` as belt-and-braces. Defence-in-depth alongside the launcher's
  `game_running` gate. Fail-safe: any error logs and lets the game run normally.
- **Clean-exit cleanup** — the claim-winner removes `game_instance.json` in
  `_ExitTree`; a duplicate never deletes the live holder's file, and a crash
  leaves it for the launcher's probe to self-correct.
- The game now reads the launcher handoff's **`data_dir`** so it writes
  `game_instance.json` into the exact directory the launcher probes; with no
  handoff (editor / standalone) it computes the same per-user dir itself,
  mirroring the launcher's `directories`-crate layout.

---

## [0.14.2] — 2026-06-20

Seats Extended-mode players by **join order** (who entered the lobby first)
instead of a `player_id` sort, so the portal layout reflects the table order
players actually see. Requires **server v0.17.0** (which sends the seating
roster); against an older server the client falls back to the v0.14.1 id-sort.

### Changed
- **Portal seats now follow join order, not id.** `GameScene.BuildEdges` reads a
  server-authoritative, frozen, self-inclusive seating roster
  (`SessionContext.SeatOrder`) — `[host, …joiners]` in the order they joined (P1 =
  the player who created the lobby) — captured once at match Start from
  `start_signaling` and, on a process-death rejoin, from the new `seat_order`
  field of the `Identified` frame. The `SeatEdge` table and the freeze/heal
  behaviour are unchanged; only the basis for *which* player takes each seat moved
  from `player_id` order to join order. Because the roster is frozen server-side
  at Start, a mid-match host promotion still never re-seats anyone, and a rejoiner
  now reproduces the identical layout (the previous id-sort happened to be
  rejoin-safe too; join order needed the server to supply the frozen order). If
  the roster is missing (older server), `BuildEdges` falls back to host-first +
  id-sort. See [`docs/architecture/extended-mode.md`](docs/architecture/extended-mode.md).

## [0.14.1] — 2026-06-19

Fixes the Extended-mode portal layout so each player's screen matches the
canonical seating diagram (`Example Imgs/GameMode Extended.png`) instead of every
player getting the same Top/Right/Left arrangement. (The seating *basis* was
later refined from this `player_id` sort to join order in [0.14.2] above.)

### Fixed
- **Portal edges are now assigned by seat, not a flat id-sort.**
  `GameScene.BuildEdges` previously sorted peers by `player_id` and filled a fixed
  `{ Top, Right, Left }` slot list in that order, so every player saw the same
  shape regardless of who they were. Players are now placed on a fixed table
  (**P1 = Host bottom, P2 top, P3 left, P4 right**) using a globally-consistent
  order (Host first, then the rest sorted by id): on your own upright screen the
  peer **opposite** you takes Top, the one on your **right** takes Right, the one
  on your **left** takes Left — reproducing the per-player layout in the diagram.
  Fewer-than-4-player rounds leave empty seats as walls, unchanged. The seating is
  decided once at Start and **frozen** for the match, so a mid-game host promotion
  never re-seats anyone or moves a portal. See
  [`docs/architecture/extended-mode.md`](docs/architecture/extended-mode.md).

## [0.14.0] — 2026-06-16

Lobby chat goes live. The chat box in the Session Lobby — previously a static
mockup — now sends and receives real messages relayed through the server, so
every player in the session sees the same conversation in the same order.
Requires server **v0.16.0+**.

### Added
- **Server-relayed lobby chat.** `SignalingClient` gains a `SendChatMessage` and
  a `ChatMessage` event over the signaling WebSocket (`send_chat` /
  `chat_message`). In `SessionLobby`, pressing **Enter** in the chat input
  (`LineEdit.TextSubmitted`) sends the trimmed message; the field clears
  immediately. Incoming messages append to the chat log as `<name>: <text>`, with
  the name resolved through the same `DisplayNameFor` fallback (`Player <id>`) the
  roster uses. The server echoes every message back to the sender too, so all
  clients render from the same broadcast rather than a local guess. Names and
  message text are added with `RichTextLabel.AddText` (not parsed as BBCode), so a
  message can't inject formatting tags.

### Changed
- The chat log's placeholder sample line is removed; the log starts empty and
  fills only from server broadcasts.

## [0.13.0] — 2026-06-16

The Session Lobby gets a visual + layout pass. Its panels now read as deliberate,
framed areas instead of the engine's default flat boxes, and the whole screen is
laid out by **percentage of the viewport** so every field stays on-screen at any
window size or aspect ratio. Purely a game-client presentation change — no
protocol, game-server, or version-requirement change. (Lobby chat is wired in a
follow-up.)

### Added
- **Bordered, defined lobby panels.** `MenuTheme.tres` gains a
  `PanelContainer` panel stylebox in the existing blue/cyan family (translucent
  navy fill, 2px light-blue border, 8px corners, subtle cyan glow) so the left
  (session info) and right (roster + chat) panels are clearly framed. A darker
  `InnerPanel` theme-type-variation gives the player-roster box and the chat box a
  recessed, nested look.

### Changed
- **Percentage-based responsive layout.** `SessionLobby.tscn`'s side panels move
  from absolute, top-anchored pixel offsets (which left the right panel only ~20px
  above the bottom buttons) to **fractional anchors**, so each panel holds its
  proportion of the screen. The roster is wrapped in an inner panel and the chat
  box now expands to fill the remaining height (replacing a greedy spacer), keeping
  the chat log and input fully visible. The four roster slots re-parent under the
  new `RosterBox/RosterMargins`; `SessionLobby.cs` slot paths follow.

## [0.12.1] — 2026-06-14

Fix: a **served ball now counts as a hit**, so serving credits the player who
served. A ball that reaches a goal was only ever credited to the last player to
deflect it with a paddle, and serving wasn't a deflection — so a clean serve that
crossed into a peer's goal untouched (or any rally where no paddle ever touched
the ball) scored nobody. The serve now tags the ball with the serving player's
`player_id` the instant it is launched, exactly as a paddle hit does; a later
paddle hit by anyone still overwrites it, so credit always follows the last player
to act on the ball. Entirely a game-client change — no protocol, game-server, or
version-requirement change.

### Fixed
- **Serve counts as applied force.** `GameScene` stamps `Ball.LastHitterId` with
  the local `player_id` at serve launch (alongside the velocity). Self-goals stay
  suppressed (a serve that returns to your own goal untouched still scores
  nobody), and the now-unreachable "untouched ball" path remains as a harmless
  guard. The simulation, handoff packet, and game-server scoring channel are
  unchanged — they already propagate and credit `LastHitterId`.

## [0.12.0] — 2026-06-13

Multiplayer UI now shows player **usernames** instead of raw player-id numbers in
the lobby roster and the in-game scoreboard. The numeric `player_id` stays an
internal key (networking, scoring, peer matching) and is never shown to players —
it appears only inside the `Player <id>` fallback when a username isn't available.
Requires server **v0.15.0+**.

### Added
- **Usernames in the lobby roster and scoreboard.** `SessionContext` keeps a
  `player_id → username` map learned from the signaling `Identified` /
  `PeerJoined` frames; `DisplayNameFor` resolves self → local username, else the
  server-provided username, else the `Player <id>` fallback. The map is additive
  within a session (a departed player who still holds points keeps their name)
  and cleared when a session starts or ends.
- **`View2D.NameResolver`** — the in-game scoreboard renders each player's
  resolved name (injected by `GameScene` from `SessionContext.DisplayNameFor`),
  still sorted by `player_id` so duplicate display names never reorder columns.

### Changed
- `SignalingClient`'s `Identified` / `PeerJoined` events now carry the
  username/usernames data parsed from the new server fields (requires server
  **v0.15.0+**).

## [0.11.0] — 2026-06-09

### Added
- **Achievements** and **Credits** buttons on the main menu (`MainMenu.tscn`).
  Achievements sits under Customization; Credits sits under Settings. Both ship
  **disabled** as placeholders following the existing menu theme — no `Pressed`
  handlers are wired yet (like the existing Solo Play / Customization buttons).
  Functionality lands in a later release.

## [0.10.0] — 2026-06-02

Fix: a **server-synced clock** for ball handoffs. Previously the handoff
fast-forward (which nudges an entering ball inward by its transit time) measured
that transit as the difference of two machines' wall clocks. As those clocks
drifted apart over a session, the gap grew until the ball entered partway down
one player's screen — the "after ~5 minutes, ball appears halfway down" bug.
Both ends now stamp the handoff in a shared server-time frame, so the skew
cancels. Requires server **v0.11.0+**.

### Added
- **`ServerClock`** (`src/net/ServerClock.cs`): pure SNTP-style offset estimator.
  Given a probe's send/receive times (monotonic `Time.GetTicksMsec`) and the
  server's reply time, it tracks `offset = server − local`, RTT-gating noisy
  samples and smoothing with an EMA. `NowMs(localTicks)` returns server-frame
  time; `Synced` reports whether an estimate exists yet.
- **Clock-sync probe in `SignalingClient`**: a small `time_sync` round-trip over
  the existing session WS — first right after identify, then every ~12s, and
  immediately again after a reconnect (the clock may have stepped). Exposes
  `ServerNowMs()` / `ClockSynced`.

### Changed
- **`NetGameController` handoff stamps/compares with `ServerNowMs()`** instead of
  `DateTimeOffset.UtcNow`. Until the clock has a sample, the receiver skips the
  fast-forward and the ball enters cleanly at the edge; the 0.5s clamp remains a
  safety net. The `BallHandoffPacket.SentTimestampMs` wire field is unchanged
  (still `int64`) — only its meaning (now server-frame ms) is.

## [0.9.0] — 2026-05-31

Feature: an **Esc-bound pause menu** for an active multiplayer match. Client-side
only — it reuses the existing server-side reconnect grace, so no server change.

### Added
- **In-match pause menu** (`src/ui/menus/PauseMenu.tscn` + `PauseMenu.cs`): Esc
  during a live round opens an overlay (styled with `MenuTheme.tres`, matching the
  main menu) showing the **session code** and three actions — **Return to
  Session**, **Exit to main menu**, and **Quit Game**. Esc again, or Return to
  Session, dismisses it. While it's open the match stays live underneath (a P2P
  round can't truly pause) but local paddle/serve input is suspended.

### Changed
- **Esc no longer instantly leaves the match.** Previously Esc sent an explicit
  `leave` (peers promoted immediately); it now opens the pause menu instead. Both
  **Exit to main menu** and **Quit Game** leave *without* a `leave` frame, so the
  server treats them as a transient drop: the slot is held for the 2-min reconnect
  window and, for a host, the 30s promotion grace runs — i.e. the leaver gets the
  same return window a dropped player does. Quit Game then closes the app. Reuses
  the server's `RECONNECT_GRACE` (120s) / `PROMOTION_GRACE` (30s) unchanged.
- Host-vs-joiner menu variants are intentionally deferred (not in this release).

## [0.8.1] — 2026-05-30

Patch: session lobby layout fix only — no behavior or networking changes.

### Fixed
- **Session lobby** — the host's "Return to Game Setup Screen" button no longer
  gets pushed to the bottom of the left panel (next to Cancel Session). It now
  sits directly below the game settings details, where it belongs. Pure scene
  reorder in `SessionLobby.tscn` (the expanding spacer now sits below the button
  instead of above it); button styling and behavior are unchanged.

## [0.8.0] — 2026-05-30

Stage 5 (client side): **process-death recovery** — a player who crashed/quit
mid-match can rejoin the live match by re-entering the session code, and the
WebRTC mesh re-establishes so balls flow again. Pairs with the matchmaking server
at **v0.10.0+**. See
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).

### Added

- **Rejoin a live match** (`src/ui/menus/JoinMenu.cs`, `src/core/SessionContext.cs`):
  entering the code of a session you belong to that's already started no longer
  errors — the Join screen re-opens the signaling WS (the server re-admits a
  still-held member), rebuilds the mesh, and drops you straight back into
  `GameScene`. `Closed 4403/4404` surfaces a friendly "you're not part of that
  match" / "no longer exists".
- **Single-peer re-mesh** (`IPeerTransport.ResyncPeer`, `WebRtcMeshTransport`):
  tears down a stale link and re-negotiates just that connection (deterministic
  offerer unchanged), without disturbing the rest of the mesh.
- **Edge healing on rejoin** (`src/game/net/NetGameController.cs`): a returning
  peer's walled-off portal is restored and `ResyncPeer`'d. A new
  `PeerReconnecting` signaling frame drives a "a player is reconnecting…" overlay
  (`src/game/GameScene.cs`); the in-game session **code is shown** so players can
  reshare it with the dropped friend.

### Changed

- **No double-serve on rejoin** (`GameScene`): a rejoining host skips the
  first-ball serve (the ball is already in play elsewhere).

### Notes

- Recovers any dropped player within the server's reconnect window (server
  v0.10.0): joiners, and a host who is demoted-but-kept after promotion (rejoins
  as a non-host). If the single ball died with the crashed process, the rejoined
  match has no ball until the planned ball-loss watchdog lands.

## [0.7.1] — 2026-05-29

### Fixed

- **Ball entered at the wrong height on non-16:9 displays** (the
  [`known-bugs.md`](docs/planning/known-bugs.md) display-aspect bug). Each client
  still sizes its arena from its own `GetViewportRect()`, but everything that
  crosses the wire or was hard-coded in pixels is now **relative to the arena**, so
  it means the same on any screen size or aspect ratio:
  - **Handoff speeds + the transit fast-forward** (`src/game/net/NetGameController.cs`,
    `src/game/net/GamePacket.cs`): the canonical `Perp`/`Tang` now travel as a
    fraction of arena **height** per second — divided by height on send, multiplied
    by the receiver's height on receive. Normalizing both components by the same
    reference dimension preserves the entry **angle** across aspect ratios.
    `BallTransform` stays unit-agnostic.
  - **Object sizes/speeds** (`src/game/GameScene.cs`): `PaddleSpeed`, `ServeSpeed`,
    `GoalGap`, `PaddleHeight`, `BallRadius` are now fractions of arena height and
    `PaddleWidth` a fraction of arena width, resolved from each client's own arena
    (values reproduce the original 2560×1440 feel).

## [0.7.0] — 2026-05-29

Stage 4 of multiplayer (client side): the game now **survives host loss** and a
transient WebSocket drop. Pairs with the matchmaking server at **v0.9.0+**. See
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).

### Added

- **WS auto-reconnect** (`src/net/SignalingClient.cs`): an unexpected socket drop
  no longer bounces straight to the menu. The client re-dials the same session WS
  and re-sends `identify` for ~30s (matching the server's host grace), emitting
  `Reconnecting` / `Reconnected`; only a deliberate close or an auth-level
  rejection (4401/4403/4404) is terminal. Handles the new `host_reconnecting` /
  `host_reconnected` frames.
- **Host-loss grace UI** (`src/game/GameScene.cs`): a "Reconnecting…" /
  "Host reconnecting…" overlay tracks the grace window. The ball keeps flowing
  over the independent WebRTC mesh while the WS reconnects.
- **Deliberate match leave**: Escape sends `Leave` then returns to the menu, so
  peers promote a new host immediately instead of waiting out the grace.

### Changed

- **`GameScene`** reacts to `HostChanged` mid-game (updates the local host
  notion). **`SessionLobby`** surfaces reconnect status on its status line — a
  lobby blip now reconnects rather than dropping to the menu.

### Notes

- Reconnect recovers a transient WS blip while the process is alive; full
  mid-game WebRTC re-meshing after a process death is out of scope (the server's
  grace-expiry → promotion handles permanent loss).
- A separate display-aspect ball-position bug is tracked in
  [`docs/planning/known-bugs.md`](docs/planning/known-bugs.md).

## [0.6.0] — 2026-05-27

Stage 3 of multiplayer: a playable **Extended-mode** round over the WebRTC
mesh from v0.5.0. See
[`docs/architecture/extended-mode.md`](docs/architecture/extended-mode.md) and
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).

Each player renders only their own screen; the ball lives on one screen at a
time and crosses to a peer through a shared edge. No solo mode this stage —
the game is entered from the lobby Start transition.

### Added

- **Simulation core** (`src/game/`): `GameState` (plain, node-free, per-screen
  data; multi-ball-ready), `GameSimulation.Step` (paddle reflection with
  hit-offset english, trigonometric wall reflection, goal/score detection),
  and `BallTransform` (frame-independent perp/tang/along handoff math).
- **Swappable 2D view** (`src/game/view/`): `IGameView` + `View2D : Node2D`
  drawing background, paddle, white `Ball.png`, the four edges colour-coded by
  kind (wall/portal/goal), and a scoreboard. A future `View3D` is a view swap.
- **Game scene** (`src/game/GameScene.*`): builds the edge map from the roster
  (peers → Top/Right/Left; bottom goal), runs the sim each physics frame, paddle
  on Left/Right arrows, serve on Space. The host serves the first ball; the
  scored-on player serves thereafter.
- **Ball handoff** (`src/game/net/`): compact binary `GamePacket` (`BallHandoff`)
  and `NetGameController` — sim handoff → directed `Send` to the one peer across
  the crossed edge; inbound → mapped onto the local entry edge and fast-forwarded
  by transit time. Names only `IPeerTransport`.
- **Server-relayed scoring**: a goal reports the last hitter to the server
  (`SignalingClient.SendReportScore`); the authoritative `ScoreUpdate` broadcast
  overwrites every client's scoreboard. Self-goals don't count.
- **Input map**: `paddle_left` / `paddle_right` (arrows) + `serve` (Space).

### Changed

- **Lobby:** on `start_signaling` the lobby now hands the live signaling socket
  + WebRTC transport to the `SessionContext` autoload (so they survive the scene
  change) and transitions into `GameScene`. The Stage-2 ping/pong heartbeat is
  removed — real game packets prove the link.

### Notes

- Requires the matchmaking server at **v0.8.0+** (server-relayed score channel).
- Deferred: multi-ball (needs globally-unique ball ids), solo/AI opponent,
  ball-speed cap, a serve gate until peers connect. See `extended-mode.md`.

## [0.5.0] — 2026-05-27

Stage 2 of multiplayer: `start_signaling` now establishes real **peer-to-peer
WebRTC DataChannel** connections. See
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).
Finish line is a DataChannel round-trip; gameplay over the transport is Stage 3.

### Added

- **`webrtc-native` GDExtension integration.** `scripts/fetch-webrtc.sh` pins
  release `1.1.0-stable` (Godot 4.1+) and extracts it into
  `client/addons/webrtc/` (git-ignored); CI fetches it before `godot --import`.
  Without it Godot's `WebRtcPeerConnection` is interface-only.
- **`IPeerTransport`** (`client/src/net/IPeerTransport.cs`) — topology-agnostic
  seam (`Send`/`Broadcast` + `PeerConnected`/`PeerData`/`PeerDisconnected`/
  `PeerFailed`) the Stage 3 game layer will consume. Topology is a
  per-game-mode strategy, so future modes can swap in a relay/SFU transport.
- **`WebRtcMeshTransport`** — full mesh of `WebRtcPeerConnection` +
  `WebRtcDataChannel` per peer; STUN-only ICE; deterministic glare rule
  (smaller `player_id` offers); buffers remote ICE until the remote
  description is set.
- **Signaling negotiation** — `SignalingClient` gains offer/answer/ice +
  `peer_connection_failed` senders and surfaces the inbound offer/answer/ice
  frames as events.
- **CI:** `scripts/godot-headless.sh` tolerates the webrtc-native headless
  teardown segfault (which fires after the export artifacts are written);
  a new "Verify WebRTC native libs bundled" gate asserts the native lib
  ships in every platform export.

### Changed

- **Lobby:** on `start_signaling` the lobby builds the WebRTC mesh and proves
  a DataChannel round-trip (ping/pong), showing "N/N connected · M echo OK".

### Known limitations

- **No TURN** — symmetric-NAT peers can't connect yet (deferred). STUN only.

## [0.4.0] — 2026-05-26

Stage 1 of multiplayer: the menu shell becomes a **working lobby over the
live server**. See [`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md)
for the full staged plan. Stops at `start_signaling` — WebRTC peer
connection and gameplay are later stages.

### Added

- **Client networking layer (`client/src/net/`):**
  - `ServerEndpoint` — base URLs from the compile-time-baked `BuildConfig`
    host. No runtime host selection — channel isolation stays at the
    build-artifact level.
  - `Dto` + `ServerApi` — snake_case DTOs mirroring `shared/`, an
    `HttpClient` wrapper over `/register`, `/host`, `/join`,
    `GET /session/:code`, `/start`, `DELETE`, and `/session/:code/host`.
    Sends `X-Game-Version` + `X-Launcher-Version`; returns typed
    `ApiResult` so the UI branches on the server's `error` code.
  - `SignalingClient` — a `Node` polling `WebSocketPeer` on the main
    thread; sends `identify`/`leave`, surfaces Identified / PeerJoined /
    PeerLeft / HostChanged / StartSignaling / SessionEnded / Kicked /
    Closed. WebRTC frames are received but not yet acted on.
- **Identity handoff:** `LaunchArgs.Handoff` now carries `player_id`,
  `secret_token`, `launcher_version`, and `channel`. `SessionContext` holds
  the identity, owns the `ServerApi`, asserts the handoff channel matches
  the build, and (DEBUG/editor only, no handoff) self-registers so two
  editor instances can test without the launcher.

### Changed

- **Host / Join / Lobby menus** now make real server calls. The lobby
  roster is driven entirely by signaling events, with manual host handoff,
  Start, Cancel/Leave, and `SessionEnded`/`Kicked`/disconnect handling.
  Replaces the F1/F2/F3 fake-player debug. Peers show as `Player <id>`
  (the server roster has no usernames yet).

## [0.3.0] — 2026-05-25

Adds a **macOS (universal) export** target — Stage B of the macOS effort. The
game now exports as an ad-hoc-signed **Universal 2** `.app` (Apple Silicon +
Intel), and the launcher knows how to install and launch it.

The export is **cross-built on the Linux CI runner**, not on a macOS runner:
`godot --headless` reliably hangs on `macos-latest` — it prints the engine
banner then never exits (orphaned Godot process), at `--import` and every other
headless entry point. A 3-variant matrix probe (`--embedded`, `--quit-after 2`,
export-implicit-import) all hung on Godot 4.6.3 / macOS 15.7.7, even though all
three upstream macOS-headless fixes (godot#108696, #113267, #113269) are present
in 4.6.3. Godot's export templates are platform-independent, so the Linux job
produces the same `.app` in seconds — and ad-hoc signing is done off-macOS with
`rcodesign`.

### Added

- **macOS export preset** (`client/export_presets.cfg`, `[preset.2]`):
  `binary_format/architecture=universal` (the official templates ship only
  `godot_macos_release.universal`, and .NET macOS builds for both arches —
  godot#94631), `bundle_identifier=com.phoenixwired.briskablast.client`. Godot's
  own signing is disabled (`codesign/codesign=0`); CI ad-hoc signs instead.
- **`project.godot` `[rendering]`**: `textures/vram_compression/import_etc2_astc=true`,
  required for any universal/arm64 macOS export (the GPU uses the ETC2/ASTC
  texture family). Harmless to the x86_64 Linux/Windows presets, which keep
  using S3TC/BPTC.
- **macOS cross-export in the Linux `build` job** (`release-client.yml`): exports
  `BriskaBlast.app`, verifies the embedded C# by asserting `BriskaBlast.dll` in
  **both** `Contents/Resources/data_BriskaBlast_macos_arm64/` and
  `…_x86_64/` (Godot's .NET macOS export lays managed assemblies out as loose
  files per-arch rather than embedding them in the `.pck` like Linux/Windows),
  installs `rcodesign` (apple-codesign 0.29.0, Linux musl prebuilt), ad-hoc
  signs the bundle, and publishes
  `briskablast-client-<channel>-<version>-macos.tar.gz` alongside the other
  platform assets. The dedicated `macos-latest` `build-macos` job was removed.
- **`client/global.json`** pins the game project to the .NET 8 SDK
  (`rollForward: latestFeature`) so the export job resolves .NET 8 (and the
  `dotnet new sln` / `dotnet sln add` step produces a `.sln`, not the newer
  `.slnx`, which Godot's export plugin can't consume).

### Launcher-side (install/launch path)

- `installer.rs`: `select_platform_asset` matches `macos.tar.gz` on macOS, and
  extraction resolves the in-bundle Mach-O
  (`BriskaBlast.app/Contents/MacOS/…`) as the manifest executable. `game_launch`
  needed no change — it spawns the manifest executable directly.

### Notes

- Ad-hoc signing is tester-grade (right-click → Open the first time, or strip
  the quarantine xattr), **not** Developer-ID / notarized for public download.
- The `.app` is CI-valid but **not yet launch-tested on real Mac hardware** —
  smoke-testing on a Mac before a public release is advised.

---

## [0.2.5] — 2026-05-24

Pure pipeline cleanup on top of v0.2.4 — **no behaviour change** to the
exported game.

### Changed

- **`release-client.yml`:** dropped the v0.2.3 `dotnet build (Debug)` step
  (and its explanatory comment block). It pre-populated `bin/Debug/` so the
  editor-mode runtime could load the project assembly during import. Once
  v0.2.4 added `BriskaBlast.sln` generation, Godot's own
  `BuildManager.PublishProjectBlocking` builds and embeds the managed DLLs,
  making the step redundant. The post-export `pck_size` ≥ 5 MiB canary still
  guards against a regression.

### Added

- **`ci-client.yml`:** mirrored the `.sln`-generation step from
  `release-client.yml`. That workflow is `workflow_dispatch`-only today, but
  if it is re-enabled on `client/**` PRs it would otherwise hit the same
  "no solution file was found" failure when headless Godot processes `.cs`
  files.

## [0.2.4] — 2026-05-24

Fourth (and hopefully last) iteration on the headless .NET export.
v0.2.3 still shipped a 64 KB `.pck`. Re-read the **verbose** Godot
output instead of trusting the verify step's surface message, and
the real error was screaming on the first export attempt — buried
mid-`savepack` so it scrolled past the first three investigations:

```
ERROR: Export .NET Project: This project contains C# files but
no solution file was found at the following path:
  /home/runner/work/Briska-Blast/Briska-Blast/client/BriskaBlast.sln
A solution file is required for projects with C# files.
```

Repeated once per `.cs` file. `GodotTools.Export.ExportPlugin._ExportFile`
bails on each, so `BuildManager.PublishProjectBlocking` never fires —
no managed DLLs in the `.pck`. Godot still exits 0 (the known silent
failure: godotengine/godot#86591, #98225), so CI marches forward and
only the post-export `pck_size` canary catches it.

### Why v0.2.1, v0.2.2, and v0.2.3 all missed this

The repo carries `client/BriskaBlast.csproj` but no
`client/BriskaBlast.sln`. On a desktop, the Godot editor auto-creates
the `.sln` the first time the project opens; in headless CI nothing
ever does. The three previous fixes all tweaked the `dotnet
publish` / `dotnet build` / `--editor --quit` ordering — none of which
produces a `.sln`. So each fix was rearranging deck chairs while
ExportPlugin kept bailing at the same earlier point.

The `.sln` is just a plain-text list of which `.csproj` projects belong
to the solution — `dotnet new sln` + `dotnet sln add` produces the same
file the editor would.

### Fixed

- **`.github/workflows/release-client.yml`**: new `Generate solution
  file` step, run right before `dotnet restore`. Runs
  `dotnet new sln --name BriskaBlast` then
  `dotnet sln BriskaBlast.sln add BriskaBlast.csproj` inside `client/`.
  Gives the export plugin the `.sln` it requires; not committed to git
  (matches the editor-side lifecycle on desktop checkouts).

### Unchanged from v0.2.3

- The v0.2.3 `dotnet build` (Debug) step is left in. It becomes moot
  once the `.sln` exists (the export plugin's own publish handles
  the assembly state), but we changed exactly **one** variable this
  iteration so the next failure — if any — has a clean attribution.
  Cleanup to a follow-up PR.
- Still no manual `dotnet publish`, no `--editor --quit` warm-up.
- `dotnet/embed_build_outputs=true` on both presets.
- `--verbose` on `--export-release`.
- Verify-pck-size canary — kept; it caught the last three failures.

### Not touched

- **`.github/workflows/ci-client.yml`** would hit the same issue if
  ever triggered (it's `workflow_dispatch`-only today, so it isn't
  blocking anything). Adding the same step there is a follow-up.

---

## [0.2.3] — 2026-05-24

Third iteration on the headless .NET export. v0.2.2's verify step
caught another empty-of-C# .pck. The verbose log was decisive:

```
.NET: GodotPlugins initialized
.NET: Failed to load project assembly      ← happens HERE, before any publish
[scan filesystem]
ERROR: Failed to create an autoload, script 'SettingsManager.cs' is not compiling.
```

The assembly load failure is in Godot's RUNTIME initialisation,
**before** the export plugin's `BuildManager.PublishProjectBlocking`
would fire. Autoload registration then cascades because the C# scripts
can't compile against a missing assembly. By the time the export
plugin runs, the project is in a broken state and the publish is
skipped — no C# in the .pck.

### Why v0.2.2 didn't reach the publish step

Godot's editor-mode runtime reads the project assembly from
`.godot/mono/temp/bin/<Configuration>/<AssemblyName>.dll`. The default
configuration when not specified is `Debug`. We weren't building Debug
at all in v0.2.2 (the maintainer-confirmed repo assumed clean bootstrap
auto-builds), so `bin/Debug/BriskaBlast.dll` didn't exist, and the
assembly load failed at runtime init.

### Fixed

- **`.github/workflows/release-client.yml`**: re-added a single
  `dotnet build` step (default Debug, no `-c` flag) right after
  `dotnet restore` and before `Pre-import resources`. Populates
  `bin/Debug/BriskaBlast.dll` so Godot's runtime can load the
  assembly during initialisation. Autoload registration then
  succeeds, and the export plugin can do its own publish for the
  export configuration.

### Unchanged from v0.2.2

- Still no manual `dotnet publish` (Godot's export plugin handles it
  once autoloads work).
- Still no `--editor --quit` warm-up (placebo).
- `dotnet/embed_build_outputs=true` in `export_presets.cfg`.
- `--verbose` on `--export-release`.
- Verify-pck-size canary — kept as the safety net.

### If this still fails

Option C from research: build artifacts locally with the Godot editor
GUI (which works), CI reduced to packaging + uploading. Headless export
on 4.6.3 in this environment may simply be broken in a way that needs
manual workaround.

---

## [0.2.2] — 2026-05-24

Iteration on v0.2.1's headless .NET export fix. The v0.2.1 workflow
added an editor warm-up + explicit `dotnet publish` + `dotnet build
-c Release` thinking these would populate the paths Godot's export
plugin needs. They didn't — the verify step (correctly) caught the
broken artifact and refused to publish. v0.2.2 reads Godot 4.6's
ExportPlugin source directly and strips the placebos.

### Why v0.2.1 didn't work

Reading `godotengine/godot@4.6` source:
- `modules/mono/editor/GodotTools/.../Export/ExportPlugin.cs` shows
  ExportPlugin **internally** runs `dotnet publish` to
  `<ProjectBaseOutputPath>/godot-publish-dotnet/<Configuration>-<RID>/`
  and reads the assembly back from there.
- `modules/mono/editor/.../Sdk.props` sets the SDK output to
  `.godot/mono/temp/bin/<Configuration>/` — `bin/Debug/` for the
  default editor and the Godot-internal `godot-publish-dotnet/`
  subdir for exports.
- Our v0.2.1 manual `dotnet publish -c ExportRelease -r linux-x64`
  wrote to `.godot/mono/temp/bin/ExportRelease/linux-x64/{,publish/}` —
  a directory Godot never reads. Same for our `dotnet build -c
  Release` (`bin/Release/` is also dead code from Godot's perspective).
- `godot --headless --editor --quit` warm-up tried to load the project
  assembly from `bin/Debug/`, found nothing (we only built Release),
  failed silently, did nothing useful.

A maintainer-confirmed working pipeline
([Stalker2106/godot-ci-test](https://github.com/Stalker2106/godot-ci-test/blob/master/.github/workflows/main.yaml))
runs only `godot --headless --export-release` on a clean checkout —
no manual `dotnet build`, no `dotnet publish`, no `--editor --quit`.

### Fixed

- **`.github/workflows/release-client.yml`**: removed the three
  placebo steps from v0.2.1 (`dotnet build --configuration Release`,
  `Editor warm-up`, `dotnet publish (Linux/Windows ExportRelease)`).
  Kept the single useful addition: `dotnet restore` to warm the NuGet
  cache, and the verify-pck-size canary. The workflow now matches the
  minimal-working pattern in the maintainer-confirmed reference repo.

### Unchanged from v0.2.1

- `dotnet/embed_build_outputs=true` on both presets (still correct).
- `--verbose` on `--export-release` (still useful for diagnostics).
- Verify steps after each export — the canary that caught v0.2.1's
  failure, kept in place to catch any future regression too.

### No game behaviour changes

- Same as v0.2.1 — this is exclusively a release-pipeline iteration.

---

## [0.2.1] — 2026-05-24

Fix-only release for the **headless `.NET` export pipeline**. v0.2.0
shipped a published Windows zip containing only `BriskaBlast.exe` and
`BriskaBlast.pck` — no managed assemblies, no sibling
`data_BriskaBlast_*` folder. The Godot runtime started, found nothing
to load, and the window closed immediately on the user's machine.

### Diagnosis

The published zip was confirmed corrupted-by-design via:

```
$ python3 -c "import zipfile; print(zipfile.ZipFile('windows.zip').namelist())"
['BriskaBlast.pck', 'BriskaBlast.exe']
```

Online research traced this to two known Godot 4.x issues:

- `godot --headless --export-release` does NOT auto-compile C# the
  way the editor path does ([Godot Forum: "no data folder created on
  export"](https://forum.godotengine.org/t/no-data-folder-created-on-export/110235),
  [Godot issue #87434](https://github.com/godotengine/godot/issues/87434)).
- Silent failures of `BuildManager.PublishProjectBlocking()` inside the
  export plugin still report exit code 0 ([Godot issue #98225 —
  "headless mono export missing dotnet assemblies"](https://github.com/godotengine/godot/issues/98225)).

So our CI happily published a runnable-but-empty artifact each game tag.

### Fixed

- **`client/export_presets.cfg`**: `dotnet/embed_build_outputs=false` →
  `true` on **both** Linux and Windows presets. Bundles the managed
  assemblies INTO the `.pck` — single-file distribution, no sibling
  `data_*` folder to lose.
- **`.github/workflows/release-client.yml`** gains three new steps per
  platform that close the silent-failure hole at every layer:
  1. **Editor warm-up** — `godot --headless --verbose --editor --quit`
     populates `.godot/mono/temp/bin/ExportRelease/<RID>/` with the C#
     build state Godot's `ExportPlugin` reads from. Per the JetBrains
     TeamCity guide on Godot .NET CI builds.
  2. **Explicit `dotnet publish -c ExportRelease -r <RID>
     --self-contained false`** before each export — does the publish
     ourselves so a silent failure of Godot's internal invocation
     cannot ship a broken artifact.
  3. **`--verbose` on the `--export-release`** so any remaining
     publish issue surfaces in the log.
- **Verify steps** after each export assert `pck_size >= 5 MiB` (the
  scaffold + menus + autoloads + embedded .NET DLLs comfortably exceed
  this; an empty-of-C# `.pck` does not). Failure halts CI with a
  `::error::` annotation so we'll never again silently publish an
  artifact lacking managed assemblies.

### Verification

- `python3 -c "import yaml; yaml.safe_load(open('release-client.yml'))"`
  parses clean.
- `cd client && dotnet build --configuration Release` clean
  (csproj untouched).
- Live verification deferred to the first CI run after merge — the new
  verify steps will be the canary.

### No behavioural changes to the game itself

- `LaunchArgs.cs`, `SessionContext.cs`, menus, theme, autoloads —
  all unchanged. Same UI, same handoff protocol, same in-memory session
  state.
- This is exclusively a release-pipeline fix.

---

## [0.2.0] — 2026-05-23

Launcher-handoff protocol. The game can now receive its display username
from the launcher via a one-shot temp JSON file, so the launcher's "Play"
button can deliver per-channel identity into the game without exposing
it on the command line. Standalone launches (game run directly, no
launcher) continue to work with the existing placeholder username.

### Added

- **`LaunchArgs.FromLauncher`** (`src/core/LaunchArgs.cs`) — parses
  `--launcher-handoff <path>` from `OS.GetCmdlineArgs()`, reads the
  JSON, **deletes the file on read**, and caches the result for the
  rest of the process lifetime. Tolerant of missing arg, missing file,
  malformed JSON — returns `null` so callers can fall back gracefully.
- **`SessionContext.LocalUsername`** (`src/core/SessionContext.cs`) —
  populated from the handoff in `_Ready()`. `StartHostSession` now seeds
  `PlayerNames` from this value instead of a hardcoded literal. Falls
  back to `"Player Username 1"` if no handoff was provided so dev runs
  outside the launcher behave unchanged.

### Handoff schema

The launcher writes (and the game consumes + deletes) a JSON object:

```jsonc
{ "username": "BlastQueen99" }
```

The schema is intentionally object-shaped — future fields like
`player_id`, `secret_token`, and `server_url` (roadmap) can be added
without breaking the contract.

### Notes

- This release is the first under the `game-v*` GitHub-Release tag
  namespace. Stage 2 of the launcher install pipeline wires the CI to
  publish artifacts when `game-v0.2.0-dev.1` is pushed.
- No multiplayer endpoints yet; this is groundwork only for the
  launcher's Install / Update / Play flow.

---

## [0.1.0] — 2026-05-21

First versioned client build. UI scaffold only — no networking yet, no
playable game scene yet. Establishes the project shape, menu navigation,
shared visual theme, and an in-memory session lobby suitable for visual
testing of host/non-host roles.

### Added

- **Godot 4.6.3 .NET project scaffold** under `client/` (`project.godot`,
  `BriskaBlast.csproj`, `.gitignore`). Targets `net8.0`; nullable enabled;
  design viewport 2560×1440 matching `BackgroundDefault.png`; window starts
  at 1280×720 for laptop-friendly first launch; canvas-items stretch mode
  with `expand` aspect so UI scales cleanly across monitor sizes.

- **Main menu** (`src/ui/menus/MainMenu.tscn`): Solo Play (disabled
  placeholder), Host Game, Join Game, Customization (disabled placeholder),
  Settings, Exit Game.

- **Host Setup menu** (`src/ui/menus/HostSetupMenu.tscn`): Basic / Advanced
  tabs, Game Mode dropdown (Extended only for now), Max Players spinbox
  (range driven by selected mode, 2–4 for Extended including host),
  Create Session → Session Lobby.

- **Join menu** (`src/ui/menus/JoinMenu.tscn`): six-character session-code
  input with uppercase normalization, Enter-to-submit, length validation,
  Return-to-main-menu.

- **Settings menu** (`src/ui/menus/SettingsMenu.tscn`) with four tabs:
  - **Display** — window mode, resolution (filtered to monitor capability),
    UI scale slider, VSync mode, FPS cap, renderer (restart required).
  - **Graphics** — MSAA, texture filtering, shadow quality, post-FX.
  - **Audio** — Master/Music/SFX/UI sliders, output device.
  - **Game** — Show FPS, Show Ping, HUD scale, text size, paddle-controls
    rebind placeholder.

  Apply button persists to `user://settings.cfg` and applies live; Return
  discards staged changes.

- **Session Lobby** (`src/ui/menus/SessionLobby.tscn`): three-column
  layout. Left — session code (placeholder `ABC123`), current settings,
  Return-to-Setup. Center — Start Session. Right — Connected Players list
  with four fixed slots, promote-to-host buttons, and a chat scaffold
  (RichTextLabel log + LineEdit input; not wired). Cancel/Leave action at
  bottom-left whose text adapts to host vs non-host role.

- **`SettingsManager` autoload** (`src/core/SettingsManager.cs`):
  ConfigFile-backed; applies display settings at startup.

- **`SessionContext` autoload** (`src/core/SessionContext.cs`): in-memory
  session state (code, mode, max players, player list, host index,
  local-is-host flag) that survives scene transitions. Placeholder data
  only — no server interaction.

- **Shared menu theme** (`src/ui/theme/MenuTheme.tres`): blue Button base
  with cyan neon-glow on hover (via StyleBoxFlat `shadow_color` +
  `shadow_size`), darker pressed, greyed disabled. Matching LineEdit
  styling for SpinBox and code input — same neon focus halo.

- **Debug helpers in Session Lobby** (`#if DEBUG` only):
  F1 toggle host view, F2 add placeholder player, F3 remove last player.
  Lets us exercise host-vs-non-host UI without networking.

- **Version tracking** — `config/version="0.1.0"` in `project.godot`,
  exposed via `BriskaBlast.Core.GameVersion.Current`.

### Notes

- No game scene yet — Start Session and Solo Play are intentional no-ops.
- No server contact yet — Host/Join flows seed `SessionContext` with
  placeholder data (`"ABC123"`, `"Player Username N"`) so the lobby UI
  can be exercised end-to-end before networking lands.
- All buttons in the host-setup → lobby flow operate purely on
  in-memory state; safe to click without a running server.
