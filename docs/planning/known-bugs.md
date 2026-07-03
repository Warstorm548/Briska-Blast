# Known bugs

Confirmed defects in shipped/current builds. Distinct from
[`roadmap.md`](roadmap.md), which tracks *deferred features and decisions* — this
file is for things that are **broken**. Each entry records the observed symptom,
the suspected cause (with code pointers), a fix direction, and status.

Resolved entries are **kept, not deleted** (marked ✅ with the fix version) — a
running record of past defects so a regression can be spotted fast if one ever
resurfaces unintentionally later.

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
