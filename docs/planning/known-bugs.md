# Known bugs

Confirmed defects in shipped/current builds that are tracked but not yet fixed.
Distinct from [`roadmap.md`](roadmap.md), which tracks *deferred features and
decisions* — this file is for things that are **broken**. Each entry records the
observed symptom, the suspected cause (with code pointers), a fix direction, and
status.

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
