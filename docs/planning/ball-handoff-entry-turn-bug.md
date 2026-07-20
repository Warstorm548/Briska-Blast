# Investigation — ball enters displaced on a TURN/distant peer's screen

> **Pick-up doc.** Active investigation, paused awaiting field logs. Read this
> top-to-bottom to resume cold. Cross-refs: [`known-bugs.md`](known-bugs.md)
> (entry "Ball enters displaced on a TURN-relayed / distant peer's screen"),
> [`../architecture/extended-mode.md`](../architecture/extended-mode.md) §"Ball
> travel (handoff)", and the two **resolved** ancestors of this bug (see
> §Lineage).

## Status (2026-07-19)

- **Diagnosis:** code-level, strong — a **biased receiver server-clock offset**
  amplified by the handoff transit fast-forward. **Not** macOS, **not** the TURN
  transport itself.
- **Diagnostic build shipped:** game **0.28.1-dev.1** (tag `game-v0.28.1-dev.1`,
  commit `9e9799e`, branch `fix/ball-handoff-entry-position`), published to the
  dev channel so testers update via the launcher. Adds logging only — **no
  behaviour/math change yet**.
- **Blocked on:** field logs from the reporter (can't test the night of
  2026-07-19). Once the logs confirm, implement **Plan B** (§Fix plan).
- **Next action:** collect logs → confirm → build the fix on a **fresh branch off
  `dev`** (this diagnostic branch is merged to dev and retired).

## Symptom (as reported)

- In a live match, on **one** player's screen the ball does not enter cleanly
  from the **top edge** — it arrives **late** and already **well inside the
  field** (deep, not at the edge).
- Only that player sees it. It happens **only when that player is on the TURN
  relay**; direct-WebRTC peers never see it.
- **One-directional:** when the ball travels *from* the affected (TURN) player
  *to* a WebRTC player, entry is **normal**. Only balls entering the affected
  player's screen are displaced.
- **Geography:** the affected player connects from **India**; everyone else is
  in the **USA**. (Their only TURN path is also their only Mac and their only
  distant player — all three are confounded in the current test setup.)

## Root-cause diagnosis

### The mechanism (code)

The receiver **fast-forwards** every incoming ball by its transit time so it
doesn't visually lag at entry — `client/src/game/net/NetGameController.cs`,
`OnPeerData`:

```csharp
long rawTransitMs = _signaling.ServerNowMs() - pkt.SentTimestampMs;
float transit = _signaling.ClockSynced
    ? Mathf.Clamp(rawTransitMs / 1000f, 0f, 0.5f)   // clamp 0–500 ms
    : 0f;
pos += vel * transit;                                // shove inward by speed × transit
```

- `transit = receiver's ServerNowMs() − sender's SentTimestampMs`, both stamped
  in a **server-synced clock frame** (`client/src/net/ServerClock.cs`), fed by an
  SNTP-style `time_sync` probe over the session WebSocket
  (`SignalingClient.MaybeSendTimeSync` / the `time_sync` case).
- The inward displacement is **speed × transit**, and `transit` is entirely a
  function of the **receiver's own server-clock offset estimate**.

### Why it's one-directional (the decisive evidence)

Say the affected receiver's clock offset is over-estimated by **Δ**:

- **A → affected:** `transit = realTransit + Δ` → **over**-fast-forward → ball
  lands deep inside (the bug).
- **affected → A:** the accurate peer computes `transit = realTransit − Δ` →
  **under**-fast-forward → ball sits ~at the edge, clamped to 0 → **looks
  normal**.

A single biased offset on **one receiver** produces displacement in exactly one
direction. Pure symmetric relay latency (good clocks) would displace **both**
directions equally — which is *not* what's observed — so latency alone is ruled
out. The defect is a **biased clock on the receiving client.**

### Why it correlates with TURN and with India (same underlying fact)

The `time_sync` offset is estimated as `offset = serverMs − (t1 + t4)/2`, which
**assumes a symmetric path** to the server (VPS). That assumption breaks on a
long-haul link:

- **India ↔ US/EU server** is ~200–400 ms RTT and routes are classically
  **asymmetric** (out ≠ back) with high jitter → the offset is **biased and
  noisy**.
- That same distant/NAT'd peer is also the one that **needs TURN** (and happens
  to be on a Mac). So "TURN player" = "the Mac" = "the India friend" are very
  likely **one person**, and the real driver is **geographic distance/asymmetry
  to the server**, not the OS or the relay.
- Before TURN existed, that symmetric-NAT peer **could never connect at all**, so
  the bug was invisible — TURN *revealed* it, didn't *cause* it.

### Two effects, both maxed out for the distant player

1. **Biased clock offset** — needed to explain the *direction* (the core defect).
2. **Genuinely large real transit** — even a *perfect* clock can't undo a real
   ~300 ms India↔US TURN hop; a fast ball legitimately travels deep in 300 ms.
   So the fix needs a **spatial cap** too, not just clock hardening.

## Specific weaknesses (fix targets)

| # | Where | Weakness |
|---|---|---|
| 1 | `ServerClock.AddSample` (first-sample branch) | **First sample seeds the offset 100%** with no history, and the first probe fires during busy mesh/TURN setup (`_nextSyncMsec` starts at 0) — the worst moment gives the seed. Recovers only at `SampleWeight=0.25` every 12 s. |
| 2 | `ServerClock.MaxRttMs = 1000` | Accepts samples with up to a **1 s** round-trip; such samples can be asymmetric by hundreds of ms → offset error up to ~±500 ms (= the fast-forward clamp). |
| 3 | `ServerClock.AddSample` (EWMA) | **No lowest-RTT / best-of-N selection.** Real SNTP keeps the lowest-RTT sample (least asymmetry); this EWMA-blends everything. |
| 4 | `ServerClock.Synced` | Flips **true after one sample** — a bad seed is trusted immediately. |
| 5 | `NetGameController.OnPeerData` fast-forward clamp | **500 ms clamp caps *time* only, not *spatial* displacement.** For a fast ball that's a large fraction of the arena regardless of clock quality. |
| 6 | `SignalingClient.SyncIntervalMsec = 12000` | Sparse: only one probe every 12 s, so early convergence is slow. |

## Fix plan (Plan B — after logs confirm)

Two parts. **The spatial cap is the robust primary fix** (bounds the worst case
regardless of clock/latency); clock hardening reduces the common-case error.

1. **Fast-forward spatial cap** (primary, low-risk, `NetGameController.OnPeerData`):
   cap the inward shove so the ball can never enter more than ~10–15 % of arena
   height inward, no matter how large `transit` is. Consider also tightening the
   0.5 s time clamp. This survives a genuinely huge *real* transit (effect #2)
   that no clock fix can undo.
2. **Clock-sync hardening** (`ServerClock` + `SignalingClient`):
   - Send a **burst** of probes at match start (e.g. 5–8 rapid) instead of one.
   - **Seed/keep the lowest-RTT sample** (or median of a window) rather than the
     first, and don't flip `Synced` true until there are N samples / low
     dispersion. Until then `transit = 0` (enter cleanly at the edge — acceptable).
   - Re-tune `MaxRttMs`. **Trade-off to weigh:** India's *legitimate* RTT to a US
     server may be ~250–400 ms, so tightening `MaxRttMs` below that would make
     India **never sync** → it would enter balls at the edge with no fast-forward
     (looks normal, slightly laggy). That may actually be an acceptable/cheap
     behaviour for very-distant peers — decide with the measured RTT in hand.

### Alternatives considered (not chosen yet)

- **Peer-path RTT** (fast-forward by measured WebRTC RTT/2 on the actual peer
  link instead of server-relative timestamps): would isolate real transit from
  clock bias, but **Godot doesn't expose ICE getStats/RTT** — would need an
  app-level ping packet. Heavier; revisit only if the spatial cap + clock
  hardening prove insufficient.

## What the diagnostic build (0.28.1-dev.1) logs

All `Log.Info` (surfaces on every channel), all tagged `TEMP diagnostic`, to be
**removed** when the fix lands.

- **`game.handoff` — `IN transit …`** (`NetGameController.OnPeerData`): per
  incoming ball → `raw` (pre-clamp transit, **signed**), `used` (clamped),
  `synced`, `push=%ofArenaH` (how far inward it was shoved), and `peer=<display
  name>`.
- **`game.handoff` — `IN`/`OUT`/`DROPPED …`**: now carry `peer=<display name>`
  (via `SessionContext.DisplayNameFor`) instead of an opaque player_id.
- **`net.clock` — `time_sync …`** (`SignalingClient`, `time_sync` case): per
  probe → `rtt`, `sample` (this probe's raw offset), `devFromEst` (how far it
  pulled from the running estimate), `smoothed`, `synced`.
- **`ServerClock.OffsetMs`**: read-only accessor added for the above.

## How to read the logs

Testers: launcher → Settings → Game Channel Management → **Logs** → grab each
machine's `.log` (one per run). Then:

```
grep -E "IN transit|time_sync" <the .log file>
```

**Confirms the diagnosis:**
- **India friend:** `net.clock` shows high `rtt` (200–400+ ms) and big/jumpy
  `devFromEst`; `IN transit` shows `raw=` near the 500 ms clamp + large `push=%`.
- **US players:** low `rtt`, tight `devFromEst`; balls incoming *from* the India
  peer show `raw=` **small or negative** (a negative transit is physically
  impossible for real latency → direct proof the clocks disagree).

**Would point elsewhere:** if the affected client's `devFromEst`/offset is
small/stable but `raw` is still large in *both* directions, it's genuinely
symmetric high latency — then the spatial cap alone is the fix and clock
hardening is unnecessary.

## Lineage — this is the 3rd of three related "ball enters partway down" bugs

1. **Non-16:9 displays** (✅ game v0.7.1) — clients ran *different arena sizes*,
   so they didn't share a frame. Fixed by normalizing wire quantities by arena
   height.
2. **Clock drift after ~5 min** (✅ game v0.10.0 + server v0.11.0) — transit was
   `receiverWallClock − senderWallClock`; drifting PC clocks inflated it. Fixed
   by stamping in a **server-synced frame** — this is what *introduced*
   `ServerClock`.
3. **This bug** — the server-clock **offset *estimate* itself** is biased on a
   long asymmetric path (distant/TURN peer), so the fast-forward over-shoots even
   with server-synced clocks. The residual that #2's fix didn't cover.

See the two resolved entries in [`known-bugs.md`](known-bugs.md).

## State / pointers

- **Branch:** `fix/ball-handoff-entry-position` (pushed).
- **Diagnostic commits:** `0f8eb00` (logging), `7ec132c` (0.28.1 bump),
  `9e9799e` (enriched: peer name + per-sample offset).
- **Tag/release:** `game-v0.28.1-dev.1` → commit `9e9799e`, published (dev,
  prerelease, all 3 platform assets).
- **No `min_game_version` change** — arena geometry unchanged from 0.28.0, so
  0.28.0 and 0.28.1 interoperate.
- **Key files:** `client/src/game/net/NetGameController.cs` (fast-forward),
  `client/src/net/ServerClock.cs` (offset math), `client/src/net/SignalingClient.cs`
  (probe + logging), `client/src/game/net/GamePacket.cs` (`SentTimestampMs`).

## Open questions

- **Where is the game server (VPS) hosted** (US/EU/other)? Whoever is farthest
  from *it* shows the bias. The friend's `rtt=` line reveals distance-from-server
  regardless, so this isn't blocking.
- After the fix: is a **spatial cap alone** enough for acceptable feel over a
  ~300 ms link, or does the clock hardening measurably help? Decide from the
  before/after logs.

## Resume checklist (next session)

1. Get the tester logs (both machines) and `grep -E "IN transit|time_sync"`.
2. Confirm the pattern above (biased/jumpy offset on India; negative `raw` on the
   US side). Update this doc + `known-bugs.md` with the measured numbers.
3. Implement **Plan B** on a **fresh branch off `dev`** (the diagnostic branch is
   merged/retired): spatial cap first, then clock hardening; **remove the
   `TEMP diagnostic` logging**.
4. Bump `client/project.godot` `config/version` (0.28.1 → next), changelog entry,
   tag `game-v<next>-dev.1`, ship, re-test with the same logs.
