# Known bugs

Confirmed defects in shipped/current builds. Distinct from
[`roadmap.md`](roadmap.md), which tracks *deferred features and decisions* — this
file is for things that are **broken**. Each entry records the observed symptom,
the suspected cause (with code pointers), a fix direction, and status.

Resolved entries are **kept, not deleted** (marked ✅ with the fix version) — a
running record of past defects so a regression can be spotted fast if one ever
resurfaces unintentionally later.

---

## A rejoining client's scoreboard reads 0 until the next point

- **Status:** open — **pre-existing**, surfaced (not caused) by the in-match
  leaderboard in game 0.34.0. Needs a server-side change, so it is not fixed here.
- **Affects:** every client that rejoins a match in progress, any version. The old
  single-line scoreboard had the same gap; a ranked board just makes it obvious.
- **Symptom:** after a process-death rejoin, the leaderboard shows every player on
  0 (ordered by seat order, since no tie stamps exist yet) until the next point is
  scored **anywhere** in the match, at which point the next `ScoreUpdate` broadcast
  resyncs the whole tally and the board corrects itself.
- **Cause:** nothing re-fetches the tally on rejoin. `GameState` is reconstructed
  empty in `GameScene._Ready`, and the `Identified` frame carries
  `host_player_id, peers, seat_order, is_host, usernames, ice_servers` — no
  `scores` field. Scores only ever arrive via the periodic wholesale `ScoreUpdate`
  broadcast, which is emitted when someone scores, not when someone rejoins.
- **Fix direction:** add the current tally to the rejoin path — either as a field
  on `Identified` or as a `ScoreUpdate` pushed to the rejoining socket on identify.
  The latter is smaller and reuses the frame clients already handle. Either way it
  is a **protocol change**, hence deferred out of the 0.34.0 client work.
- **Workaround:** none needed in practice — it self-corrects on the next point.

---

## ✅ Chat input loses focus after every send (lobby and match)

- **Status:** ✅ **resolved in game 0.33.0** (`fix/chat-focus-and-cursor`).
- **Affects:** game 0.32.0, both surfaces — the bug was in the shared
  `ChatPanel`, which the lobby and the match both render through.
- **Symptom (as reported):** press Enter to send; the message posts and the box
  clears, but the caret disappears and the focus ring drops. Anything typed next
  goes nowhere until the field is clicked again or Enter is pressed a second
  time.
- **Cause:** nothing in the client released focus on that path — there are
  exactly four focus calls in `client/src` and none of them fire on a send, and
  `SessionLobby.Render` only sets `Text`/`Visible`. The input surrenders focus as
  it consumes the submit, below client code.
- **Fix:** re-grab focus after posting, **deferred** — a same-frame grab is undone
  by the release that follows it (`ChatPanel.OnSubmitted`). The intentional
  release on an empty box or a bare `/` is untouched: that is the way out of chat.
- **Regression watch:** if this resurfaces, check first that the re-grab is still
  deferred rather than direct — that is the part that is easy to "simplify" away.

## ✅ Mouse usable during a match; clicking chat silently froze the paddle

- **Status:** ✅ **resolved in game 0.33.0** (`fix/chat-focus-and-cursor`).
- **Affects:** game 0.32.0. Before it, no build ever set `Input.MouseMode` at all
  — the cursor was simply always visible, everywhere.
- **Symptom:** the cursor was visible for the whole match, and clicking the chat
  panel focused the input. Because a focused input sets the `_chatFocused` latch,
  that suspended the paddle, the serve and the hotbar keys with nothing on screen
  to explain why the controls had stopped responding.
- **Cause:** two independent gaps. Nothing hid the cursor, and `ChatPanel.tscn`
  set no `mouse_filter`, so every control in it accepted clicks.
- **Fix:** `GameScene.UpdateCursor` hides the cursor for live play and reveals it
  only for the pause menu and the end screen; `InGameChat` calls
  `ChatPanel.MakeClickThrough()` so clicks pass through the in-match panel. Both
  halves are needed — a hidden cursor still delivers clicks. The lobby is
  unchanged.
- **Regression watch:** `Input.MouseMode` is **global**, so the unconditional
  restore in `GameScene._ExitTree` is what keeps menus usable after a match. A new
  in-match overlay with clickable controls must be added to `UpdateCursor`'s rule.

---

## Ball enters displaced on a TURN-relayed / distant peer's screen

- **Status:** open — **under active investigation**; code-level diagnosis strong,
  **awaiting field logs to confirm** before the fix. A **diagnostic-only** build
  (game **0.28.1-dev.1**, branch `fix/ball-handoff-entry-position`) is shipped to
  the dev channel to capture the numbers. Full write-up + fix plan:
  [`ball-handoff-entry-turn-bug.md`](ball-handoff-entry-turn-bug.md).
- **Affects:** any match with a peer on a **long, asymmetric path to the server**
  (reported: a player in **India** vs. others in the USA) — in practice the same
  peer that needs the **TURN relay** (game v0.21.0+/server v0.22.0+). Direct
  WebRTC peers on short paths are unaffected.
- **Symptom (as reported):** on the affected player's screen the ball enters
  **late and already deep inside the field** instead of cleanly at the top edge.
  **One-directional** — balls the affected player *sends* arrive normally on a
  WebRTC peer; only balls entering *their* screen are displaced.
- **Cause (suspected):** the receiver fast-forwards each incoming ball by its
  transit time — `pos += vel * transit`, `transit = ourServerNowMs −
  pkt.SentTimestampMs` clamped 0–500 ms (`NetGameController.OnPeerData`), both
  stamped in the `ServerClock` server-synced frame. The SNTP offset estimate
  (`offset = serverMs − (t1+t4)/2`, `ServerClock.AddSample`) **assumes a symmetric
  path**; a long/asymmetric link (distant player → server) **biases it**, so the
  receiver **over-fast-forwards** incoming balls (deep entry) while the balls it
  sends are **under-fast-forwarded** by the accurate peer (clamped to the edge =
  looks normal) — which is exactly the one-directional symptom. Aggravated by:
  first sample seeds the offset 100 % during busy mesh setup, `MaxRttMs=1000` is
  generous, no lowest-RTT selection, and the fast-forward clamps *time* not
  *distance*. **Not** macOS and **not** the TURN transport itself — TURN merely
  *reveals* it (that peer couldn't connect at all before TURN existed).
- **Fix direction (planned, not yet built):** (1) **spatial cap** on the
  fast-forward so the ball can never enter more than ~10–15 % of arena height
  inward regardless of `transit` (robust against both a biased offset *and* a
  genuinely huge real transit); (2) **clock-sync hardening** — burst probes at
  start, seed from the **lowest-RTT** sample, require convergence before
  `Synced`, re-tune `MaxRttMs`. See the write-up for the RTT trade-off. This is
  the **3rd in a lineage**: it's the residual of the clock-drift fix below that
  the server-clock *offset estimate* itself doesn't cover.
- **Related (resolved ancestors):** "Ball appears partway down the screen on
  non-16:9 displays" (✅ v0.7.1) and "…after several minutes (clock drift)"
  (✅ v0.10.0, which introduced `ServerClock`) — both below.
- **Workaround:** none yet; direct-WebRTC (short-path) players are unaffected.

---

## Update could be redirected to a different install directory

- **Status:** ✅ resolved 2026-07-01 (launcher v0.17.2). The install/update prompt
  (`launcher/src/ui/center/install_prompt.rs`) shares one view for fresh installs and
  updates; its "Choose…" directory picker was always enabled. On an **update** of an
  already-installed channel a user could point the download at a *different* folder,
  stranding the old install or corrupting the update.
- **Affects:** launcher ≤ v0.17.1, any channel that had already been installed once.
- **Symptom:** updating an installed channel let the user browse to and confirm a new
  install directory instead of the existing one.
- **Fix:** "Choose…" is now disabled and the path is locked to the existing install
  location whenever the channel is already installed (keyed off
  `ChannelCreds::parsed_installed_version()`). It re-enables for a genuine first-time
  install and again after an uninstall (which clears the stored location + version). The
  backend already reused/re-validated the prior root, so this is a UI-lock only. See
  [`../launcher/launcher-foundation.md`](../launcher/launcher-foundation.md) §5F.

---

## Launcher self-update fails with "os error 5" (ACCESS_DENIED) in Program Files

- **Status:** open — root-caused and reproduced in the field 2026-07-03; the durable
  code fix (elevation fallback / clearer error) is **deferred**, not yet built. The
  affected machine was recovered by reinstalling **outside** `C:\Program Files\` plus
  a reboot.
- **Affects:** Windows, machines where the launcher is installed to its default
  `C:\Program Files\BriskaBlast\Launcher` location and run unelevated. Seen on one
  specific Win11 laptop; other Win11 machines self-updated fine.
- **Symptom (as reported):** the in-app launcher **self-update** fails with a message
  ending in **"os error 5"** and never applies, while **game** updates work fine on
  the same machine. An uninstall + manual reinstall of the newer launcher fixes it
  **once**, but the *next* self-update fails again. Adding the launcher/game to the
  **antivirus exclusions did not help.**
- **Cause:** os error 5 = Windows `ERROR_ACCESS_DENIED` — a permission/lock failure,
  **not** an AV signature hit (which is why an AV *exclusion*, which only stops
  quarantine, changes nothing). Self-update uses the `self_update` crate's
  rename-trick: the *running, unelevated* launcher renames its own
  `briskablast-launcher.exe` and drops the new binary in its place, **inside its own
  install dir** (`launcher/src/updater/github.rs:85`,
  `launcher/src/app/handlers/launcher_update.rs:50`). The NSIS installer puts that dir
  at `C:\Program Files\BriskaBlast\Launcher` (`tools/installer/launcher.nsi:17`,
  `RequestExecutionLevel admin`), which a normal user cannot write to
  (`launcher/src/paths.rs:9-13`). So the **game** update (a user-writable, user-chosen
  folder) succeeds while the **launcher** self-update (Program Files, unelevated) is
  denied. Uninstall/reinstall works only because the NSIS installer **elevates** (UAC);
  the in-app self-update never elevates, so the next update hits the same wall. On the
  one affected laptop the write was blocked where other machines' weren't — a
  locked-down / Program-Files ACL, a stale file handle on the old exe (orphan
  `.__relocated__.exe` / a security scan / a pending-rename cleared by the reboot),
  and/or **Controlled Folder Access** (Ransomware protection), whose allow-list is
  **separate from AV exclusions** and which blocks the write even when elevated.
- **Fix direction (durable):** detect access-denied / os-error-5 in the self-update
  result (`launcher/src/app/handlers/launcher_update.rs:79-82`) and **fall back to
  relaunching the elevated NSIS installer** — reuse the one-shot `runas` /
  `ShellExecuteExW` elevation already in `launcher/src/firewall.rs` — or at minimum
  surface a clear, actionable error ("your launcher folder isn't writable / Controlled
  Folder Access is blocking it — reinstall outside Program Files") instead of a raw
  "Update failed: … (os error 5)". Longer term, consider defaulting the install to a
  **per-user** location so unelevated self-update always works.
- **Workaround:** reinstall the launcher **outside** `C:\Program Files\` (a
  user-writable folder, e.g. `C:\Users\<name>\BriskaBlast\Launcher` via the NSIS
  directory page) and reboot to clear any stale lock. If it still fails from a
  user-writable folder, check **Controlled Folder Access** and add
  `briskablast-launcher.exe` to its allow-list. The launcher logs to stdout only
  (`launcher/src/main.rs:47-54`) — run the exe from a console to see the exact denied
  path.

---

## Game closes before the main menu (missing `.NET` runtime DLLs / AV quarantine)

- **Status:** open — **mitigation + triage shipped** in game/launcher 0.17.0
  (Reset Runtime Cache); the durable fix (code-signing the Windows build) is still
  pending. Confirmed via a desktop-vs-laptop folder diff 2026-06-23.
- **Affects:** Windows, some machines only (aggressive antivirus). Not all installs.
- **Symptom:** the game exits before the main menu; logs show "Failed to load hostfxr".
- **Suspected cause:** the self-contained `.NET` runtime extracts to
  `%LOCALAPPDATA%\data_<name>_windows_x86_64` on first launch. On affected machines the
  **native** runtime DLLs (`hostfxr` / `coreclr` / `clrjit`) are absent while the managed
  ones are present — the signature of **antivirus quarantining the unsigned extracted
  binaries**. GPU/resolution/single-instance/missing-system-`.NET` were all ruled out.
- **Mitigation (0.17.0):** Settings → Game Channel Management → **Reset Runtime Cache**
  (Windows) deletes the cache so the game re-extracts a clean copy on next launch. If the
  AV re-quarantines, the user must add an **AV exclusion** for the game folder themselves
  (the launcher never touches AV). See
  [`runtime-cache-and-integrity.md`](../architecture/runtime-cache-and-integrity.md).
- **Fix direction (durable):** **code-sign the Windows build** so AV stops quarantining
  the extracted binaries in the first place. Reset + exclusion only paper over it per-machine.
- **Workaround:** restore the quarantined files (or add the AV exclusion) and use Reset
  Runtime Cache; or run on a machine without aggressive AV.

---

## Ball appears partway down the screen on non-16:9 displays

- **Status:** ✅ resolved 2026-05-29 (game v0.7.1). Fixed with **percentage/relative
  coordinates** rather than the fixed-design-size direction floated below: each
  client keeps its own `GetViewportRect()` arena, but every wire quantity and
  hard-coded size is now relative to it — handoff speeds + the transit fast-forward
  cross the wire as a fraction of arena **height** per second (both components by
  the same reference, so the entry angle survives any aspect ratio), and object
  sizes/speeds in `GameScene` are fractions of the arena. Awaiting on-device
  confirmation on two different-aspect displays.
- **Status (original):** open — needs on-device reproduction to confirm the exact mechanism.
- **Affects:** game v0.6.0+ (Extended-mode round). Reported with **2 players**
  connected.
- **Symptom (as reported):** the player whose native display is **not** 2560×1440
  sees the ball appear roughly **half to three-quarters of the way down** from the
  top edge of their screen (rather than entering cleanly from the top portal).
- **Suspected cause:** `GameScene._Ready()` derives the simulation arena size from
  `GetViewportRect().Size` (`client/src/game/GameScene.cs`) — the paddle, goal gap
  and edge positions are all built from it. Under the project's `canvas_items`
  stretch mode with `aspect = "expand"` (`client/project.godot`), that size is the
  **display-aspect-dependent** logical viewport, which only equals the 2560×1440
  design space when the window is exactly 16:9. On any other aspect (16:10 laptops,
  ultrawides, a non-16:9 window) the two players run **different arena dimensions**,
  so they don't share a coordinate frame. The handoff math
  (`BallTransform.FromCanonical` + `NetGameController.OnPeerData`) preserves a
  normalized along-fraction but reconstructs the ball's entry against the
  *receiver's* arena and a transit-time fast-forward in absolute pixels — so a
  mismatched arena places the ball at the wrong vertical position on the
  non-2560×1440 player's screen.
- **Fix direction (later):** drive the simulation off a **constant 2560×1440 design
  size** (a shared constant, not `GetViewportRect()`), so every peer integrates and
  hands off in the same coordinate frame. The `canvas_items` stretch system already
  scales that single design space to fit any window/aspect, so rendering is
  unaffected — only the simulation should stop reading the per-machine viewport.
  Confirm with two clients on different-aspect displays before and after.
- **Workaround:** run both clients at a 16:9 resolution/window until fixed.

---

## Ball appears partway down the screen after several minutes (clock drift)

- **Status:** ✅ resolved 2026-06-02 (game v0.10.0 + server v0.11.0). The handoff
  now timestamps in a **server-synced time frame** (`client/src/net/ServerClock.cs`
  fed by a `time_sync` probe in `SignalingClient`) instead of each machine's wall
  clock, so the transit fast-forward measures real network delay and the
  per-machine skew cancels. Awaiting on-device confirmation over a >5-minute
  session.
- **Affects:** game v0.6.0–v0.9.x. Reported with **2 players**, only after the
  match had run **at least ~5 minutes** — distinct from the non-16:9 bug above,
  which is present from the very first handoff.
- **Symptom (as reported):** after a while, **one** player's screen shows the ball
  entering roughly **halfway down** from the top portal instead of cleanly at the
  edge; the other player's screen looks fine.
- **Cause:** the fast-forward in `NetGameController.OnPeerData` computed transit as
  `receiverWallClock − senderWallClock`, each from its own `DateTimeOffset.UtcNow`.
  That difference is `realTransit + clockSkew`; as two unsynchronized PC clocks
  drift apart (or one is stepped by NTP/sleep), the skew grows until every handoff
  fast-forwards toward the 0.5 s clamp — and only the player whose clock runs ahead
  sees it, hence "one screen." Ball speed is constant and `View2D` is
  allocation-free, so neither a speed-up nor frame drops were involved.
- **Fix:** server-anchored clock sync (option 3b) — see the game v0.10.0 changelog.

---

## A served ball that reaches a goal untouched scores nobody

- **Status:** ✅ resolved 2026-06-14 (game v0.12.1). The serve now tags the ball
  with the serving player's `player_id` at launch (`GameScene._PhysicsProcess`,
  the `serve` branch), so serving counts as applied force exactly like a paddle
  hit. Everything downstream was already correct — a later paddle hit overwrites
  the id, the handoff carries it across screens, and the goal check credits it —
  so this was the only missing link. Verified by reasoning + a clean build;
  awaiting on-device confirmation of a serve scoring into an opponent's goal.
- **Affects:** game builds with the playable Extended-mode round before v0.12.1
  (≈ v0.6.0–v0.12.0).
- **Symptom (as reported):** when a ball enters another player's screen and goes
  into that player's goal **without being hit by a paddle** — e.g. when no paddle
  moved on either screen — no point is awarded to anyone, in particular not to the
  player who served/sent it.
- **Cause:** `Ball.LastHitterId` was only ever set by a paddle deflection
  (`GameSimulation.StepBall`, the paddle branch) — serving launched the ball
  without tagging it. The goal check treats an empty `LastHitterId` as an untouched
  ball and emits a `ScoreEvent("")` (`GameSimulation.StepBall`, the
  `ball.Pos.Y >= h` block), which `NetGameController.ReportScore` then drops (empty
  scorer ⇒ nothing sent over the session socket). So any ball that reached a goal
  without a paddle ever touching it — every clean serve included — credited nobody.
  The game server's scoring channel was fine; it simply never received a report.
- **Fix:** treat the serve as applied force — set `_serveBall.LastHitterId =
  _state.LocalPlayerId` at the serve press, alongside the launch velocity.
  Self-goals stay suppressed (a serve bouncing back into your own goal still scores
  nobody). See the game v0.12.1 changelog and
  [`../architecture/extended-mode.md`](../architecture/extended-mode.md) (Scoring).

---

## Extended-mode portals don't match the seating diagram

- **Status:** ✅ resolved 2026-06-19 (game v0.14.1); seating basis refined to
  **join order** 2026-06-20 (game v0.14.2 + server v0.17.0). `GameScene.BuildEdges`
  seats players at a fixed table — **P1 (bottom), P2 top, P3 left, P4 right** — and
  assigns each peer's edge via a `SeatEdge[localSeat, peerSeat]` lookup: the peer
  opposite you → Top, right-hand → Right, left-hand → Left. Seat order is the
  server's frozen `seat_order` roster (`[host, …joiners]` in join order; P1 = the
  player who created the lobby), snapshotted at `/start` and delivered via
  `start_signaling`/the `Identified` frame; v0.14.1 originally used a `player_id`
  sort. The seating is read once in `_Ready` at Start and **frozen** for the match,
  so a mid-game host promotion never re-seats anyone (`OnHostChangedInGame` updates
  the host notion only — it does not re-run `BuildEdges`), a dropped peer still
  heals back to its **same** edge (`NetGameController._peerHomeEdge`), and a
  process-death rejoiner reconstructs the identical layout from the server snapshot
  even if a promotion happened while it was gone. Verified by reasoning against the
  diagram + clean server/client builds; awaiting on-device confirmation of a 3–4
  player match (incl. a rejoin).
- **Affects:** game builds with the playable Extended-mode round before v0.14.1
  (≈ v0.6.0–v0.14.0).
- **Symptom (as reported):** every player's screen showed the **same** Top/Right/
  Left portal arrangement regardless of where they sat, so the portals didn't line
  up with the canonical seating in `Example Imgs/GameMode Extended.png` (e.g. P3
  should see P4 on Top, P1 on Right, P2 on Left).
- **Cause:** the old `BuildEdges` collected present peers, sorted them by
  `player_id`, and filled a flat `{ Top, Right, Left }` slot list in that order —
  a per-client id-sort with no notion of seating, so the layout was identical for
  everyone and unrelated to the table geometry.
- **Fix:** seat-relative assignment via the `SeatEdge` table (see above) and
  [`../architecture/extended-mode.md`](../architecture/extended-mode.md). The
  fewer-than-4-player cases are unchanged (2 → 1 Top portal + 2 walls, 3 → 2
  portals + 1 wall). See the game v0.14.1 changelog.
