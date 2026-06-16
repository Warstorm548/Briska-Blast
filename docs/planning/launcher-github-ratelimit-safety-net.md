# Launcher — GitHub Rate-Limit Safety Net (and deferred footprint work)

**Status:** **Part 3 (the safety net) implemented in launcher v0.14.0** — see
[LauncherChangeLog.md](/LauncherChangeLog.md). The **footprint reductions**
(Part 4) remain deliberately deferred.

This doc captures the analysis and decisions from the 2026-06-15 design session
so the work can be picked up cold.

---

## TL;DR

- The launcher discovers updates by hitting **GitHub Releases unauthenticated**,
  which puts every user on GitHub's **60 requests/hour, per public IP** limit.
- Two separate problems fell out of this:
  - **A — Footprint:** how many requests each launch costs (today ~6). *Deferred.*
  - **B — Behaviour at the wall:** what happens when a user is rate-limited.
    *This is what we build first.*
- **Build now:** a persistent, header-driven **back-off safety net** so that once
  a user hits the limit, the launcher goes quiet until it resets — guaranteeing we
  never escalate from a harmless hourly throttle into GitHub's *secondary* abuse
  limits or an IP block.

---

## Part 1 — How the launcher hits GitHub (the cost model)

Discovery uses the `self_update` crate's `ReleaseList` against
`Warstorm548/Briska-Blast`, **unauthenticated** (only a `User-Agent`, no token —
and we can't ship one in a public client).

Key mechanics:

- There is no "give me the latest matching release" call we can use (the repo
  mixes `launcher-v*`, `game-v*`, `server-v*` tags across three channels), so the
  code fetches the **full `/releases` list** and filters client-side.
- `ReleaseList::fetch()` **paginates** the list at 30/page, following
  `Link: rel="next"`. So **one check = ⌈total releases ÷ 30⌉ requests.**
- At **~44 releases (2026-06-15)** that's **2 requests per check.** It grows as
  releases accumulate (dev `-dev.N` tags fastest).

Per-action cost at current size:

| Action | List-fetches | API requests |
|---|---|---|
| Boot — returning normal user (launcher self-update + Stable + EA) | 3 | ~6 |
| Boot — dev-flagged user (+ Dev after register handshake) | 4 | ~8 |
| Each manual "Check for Updates" press | 1 | ~2 |
| Binary install/update | asset-resolution call (1), then CDN redirect | ~1 |

Notes:
- The binary download hits the GitHub **asset API endpoint** (counts as 1), which
  302-redirects to the CDN; the actual byte transfer does **not** count against
  the core limit.
- `register` / reachability calls go to **our own game server**, not GitHub.
- Footprint is **O(number of users)** today — it scales toward the cap as the
  playerbase grows; **shared IPs** (campus / corporate / CGNAT / a household)
  pool the single 60/hr bucket.

Relevant code: `launcher/src/updater/branches/github.rs` (game discovery),
`launcher/src/updater/github.rs` (launcher self-update),
`launcher/src/updater/branches/installer.rs` (binary download, asset endpoint),
`launcher/src/app/mod.rs` (`boot()` fan-out), `launcher/src/app/handlers/install.rs`.

---

## Part 2 — Rate-limit fundamentals & escalation tiers

GitHub identifies unauthenticated callers by **IP**, so the 60/hr bucket is
shared by everyone behind that IP.

**Tier 1 — exceeding the primary limit (the normal wall).** Returns `403`/`429`
with `X-RateLimit-Remaining: 0` and `X-RateLimit-Reset`. It's a dumb fixed-window
counter: it resets each hour with **no accumulating penalty**. Repeatedly bumping
this wall, on its own, costs nothing beyond "checks fail until reset."

**Tier 2 — secondary / anti-abuse limits.** Separate mechanism, triggered by
*patterns*: bursting, high concurrency, and — critically — **ignoring the back-off
signal and continuing to send while limited.** Returns `403` + `Retry-After`, and
the block can be **longer** than the hourly window; continuing can extend it.

**Tier 3 — sustained egregious abuse → temporary IP block.** Rare, reserved for
clearly abusive automated patterns. Realistically only reachable by a **shared IP**
full of non-backing-off launchers. The sting is collateral: it affects *everyone*
on that network's GitHub access, not just the launcher.

**Reassurances:**
- **The repo/account is not at risk.** The traffic is unauthenticated from *users'*
  IPs; GitHub attributes it to those IPs, not to the project owner. The throttling
  lands on the users' side.
- A single normal user almost never leaves Tier 1; the launcher doesn't burst,
  loop, or auto-retry. The real risk is dense shared-IP populations **plus** the
  fact that the launcher currently **doesn't back off** (it doesn't read the reset
  and stay quiet) — that "keep knocking" behaviour is exactly what the safety net
  removes.

---

## Part 3 — The safety net (IMPLEMENTED — launcher v0.14.0)

**Scope:** graceful back-off only. **Goal:** once we know we're rate-limited, send
**zero** further GitHub-counting requests until the window resets — so we can never
climb out of Tier 1 into Tier 2/3. This does **not** reduce happy-path footprint
(that's Part 4).

### State

A new **`ratelimits.json`** in the launcher data dir (next to `identity.json`;
follow the `paths.rs` / `identity.rs` pattern), holding:

- `reset` — epoch timestamp when the limit refills (`X-RateLimit-Reset`).
- `remaining` — last-known IP budget (`X-RateLimit-Remaining`).

### The gate (runs before every GitHub-counting request)

Reading the gate is a **local file read — no GitHub request**, so it's always free.

- If a `reset` is stored and `now < reset` (+ pad) → **do not send**; surface
  "GitHub limit reached — checks resume at HH:MM".
- If `now ≥ reset` → clear/ignore the entry, resume normally.
- File missing/corrupt/unreadable → **fail OPEN** (allow). A bad file must never
  permanently brick update checks.

### Layer A — reset gate (the core guarantee)

On a **confirmed** rate-limit response (`403`/`429` with `remaining: 0`): store
`reset`, then block all counted requests until **`reset` + 2-minute pad**.

### Layer B — proactive stop

Read `X-RateLimit-Remaining` on *every* response and store it. When
**`remaining ≤ 5`**, go quiet *before* the next request would 403 (block until
`reset` + 2 min). Uses GitHub's real, IP-accurate number (sees other users on a
shared IP), so it makes Layer C unnecessary.

### Layer C — dumb local cap → **dropped**

Not built. Every GitHub API response (200 or 403) carries the rate-limit headers,
so A+B always have fresh numbers; a request that gets *no* response never reached
GitHub and didn't count. There's no realistic path where we burn budget without
seeing headers, so a hard local counter guards a door that can't open.

### No-header fallback

If a **confirmed** rate-limit response (`403`/`429`) somehow lacks a `reset`
header, apply a fixed **1-hour** cooldown. (Near-dead path on public GitHub.)

### Correctness guardrail (do not get this wrong)

Trigger the cooldown **only on a confirmed `403`/`429` rate-limit status — never on
a generic network error/timeout.** A Wi-Fi blip must not cause a 1-hour lockout.
This requires reading the **HTTP status + headers**, which is why the prerequisite
below exists.

### Gate scope

Gate **every action that spends a counted GitHub request:**
- launcher self-update check,
- per-channel game `latest_release` checks,
- the binary-download **asset-resolution** request (counts as 1).

Do **not** gate actions that make no GitHub request (register / reachability →
our server; uninstall / verify / saves folder / firewall → local). Gating a
user-initiated install is intentional: if limited, a clean "resumes at HH:MM"
beats letting the install start and die mid-flight on a 403.

### Decision summary (locked)

| Item | Setting |
|---|---|
| Layer A — reset gate | ✅ |
| Layer B — proactive stop | ✅ at `remaining ≤ 5` |
| Layer C — dumb cap | ❌ dropped |
| Clock-skew pad | **2 minutes** |
| No-header fallback | fixed **1 hour** |
| Gate scope | any counted GitHub request (checks + asset download) |
| Trigger | confirmed `403`/`429` only, never network errors |
| File-missing/corrupt | fail **open** |

### Prerequisite — "own the request"

`self_update`'s `ReleaseList::fetch()` returns releases-or-an-opaque-error and
**throws away the HTTP status + headers**, so we can't read `X-RateLimit-Reset` /
`-Remaining` or distinguish rate-limiting from a network error. The safety net
therefore needs a **minimal direct `reqwest` call** that exposes the status + the
two rate-limit headers. **Only that minimum is in scope here** — `per_page=100`,
ETag, and fetch-once (Part 4) are explicitly out of scope for this work, even
though the same refactor enables them.

### Known limitations (revisit — shipped in v0.14.0)

Three rough edges were accepted to keep the v0.14.0 scope tight. None is a
correctness bug; all are worth a follow-up pass:

1. **Disabled install button on a cold-gated boot.** If the gate is already closed
   at launch (a prior session hit the wall), boot discovery is skipped, so a
   channel whose available version was never learned leaves the bottom-bar
   Install/Update button **disabled** (it has nothing to offer). The user then sees
   a dead button rather than the "resumes at HH:MM" explanation — that message only
   surfaces on the *manual* Check buttons and inside the install prompt, which they
   can't open. *Revisit:* surface the gate state on the bottom-bar button or a small
   top-bar banner so a rate-limited boot explains itself even with no cached version.
   (Today the explanation is reachable via either "Check for Updates" button.)

2. **Resume time is local `HH:MM` only (no date / relative form).** Fine while the
   block is GitHub's hourly window (resume is always < ~1h out, so `HH:MM` is
   unambiguous). It would read poorly if the **1-hour no-header fallback** or a long
   **secondary-limit** block ever pushed the resume across midnight or well past an
   hour. *Revisit:* if those paths become non-rare, show a relative form ("~37 min")
   or include the date.

3. **`ratelimits.json` read-modify-write is not atomic across the concurrent
   fan-out.** `note_response` now reads the current block and preserves an active
   one rather than blindly clearing it (`resolve_blocked_until`), which fixes the
   stale-out-of-order *clear*. But the read→compute→write is still not atomic, so a
   vanishingly narrow race remains: if a block is armed by another task *between*
   this task's read and its write, a healthy response could still clobber it. It's
   best-effort local cache state (the gate fails open anyway), so this was judged
   not worth a lock. *Revisit:* only if it ever proves to matter in practice —
   close it with file locking or a single-writer task that owns the file.

---

## Part 4 — Footprint reduction (Problem A) — partially shipped

`per_page=100` shipped in **launcher v0.14.1**; the rest below is still deferred,
captured so it isn't re-derived. All of these ride the same "own the request"
prerequisite as the safety net.

| Idea | Effect |
|---|---|
| **`per_page=100`** ✅ shipped v0.14.1 | One page instead of 30 → kills pagination (46 releases: 2 → 1 request/check). |
| **Fetch the list once, share it** | Launcher self-update + all channels hit the *same* repo's `/releases`; one fetch serves them all. |
| **ETag / `If-None-Match`** | A `304 Not Modified` doesn't count against the limit → warm rechecks are free. |
| **Delete old releases** | Stopgap only — helps solely below 30 total releases, temporary, made moot by `per_page`. Best deletion candidates: old `server-v*` dev releases (the launcher pages past them but never uses them). |

**The punchline:** `per_page=100` + fetch-once collapse boot from **~6 requests to
~1** (one list fetch covering the self-update check and every channel). That alone
likely dissolves the rate-limit problem at current scale and demotes the safety net
to defense-in-depth — but the safety net is still worth having, and is cheaper to
ship first.

### The eventual permanent fix (bigger project)

- **Version A — server publishes metadata (recommended eventual fix):** the server
  keeps the "latest version per channel/platform" (it already owns version policy
  in Redis + the admin panel, and is already per-channel), refreshed by an **O(1)**
  poll or a release-Action push. The launcher (which already talks to the server)
  asks *it* instead of GitHub → footprint becomes **O(1) regardless of playerbase
  size.** The download URL can still point at GitHub's free CDN; that URL is the
  switch to self-hosting later.
- **Version B — self-host the binaries (optional; control/resilience, not rate
  limits):** right substrate is **object storage (S3 / R2 / B2 / MinIO) + a CDN** —
  **not** SQLite (large blobs hit ~1 GB caps, whole-blob-in-RAM reads, single-writer
  locking, and **network-FS corruption** if "shared between servers"), and **not**
  served by the game servers (that couples download bandwidth to game-server
  capacity — the "which server handles downloads?" problem dissolves with object
  storage). Cost: egress bandwidth + storage + a single point of failure vs
  GitHub's free global CDN.

---

## Related

- [`../launcher/launcher-update-and-version-validation.md`](../launcher/launcher-update-and-version-validation.md) — launcher update / version-enforcement flow.
- [`../launcher/launcher-foundation.md`](../launcher/launcher-foundation.md) §7 — update model.
- [`../dev/release-tagging.md`](../dev/release-tagging.md) — release namespaces the discovery filters on.
- [`roadmap.md`](roadmap.md) — deferred-work index.
