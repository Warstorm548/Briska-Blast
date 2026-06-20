# Game Changelog

All notable changes to the Briska Blast game are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> Renamed from `ClientChangeLog.md` on 2026-05-23 to match the
> `game-v*` release-tag namespace and avoid confusion with launcher /
> server changelogs. Content prior to the rename is preserved below.

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

Fixes the Extended-mode portal layout so each player's screen matches the
canonical seating diagram (`Example Imgs/GameMode Extended.png`) instead of every
player getting the same Top/Right/Left arrangement.

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
