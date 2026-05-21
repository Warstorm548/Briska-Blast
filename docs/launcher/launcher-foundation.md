# Launcher Foundation

Design reference for the BriskaBlast launcher (Rust + Iced). Captures layout, identity model, channel gating, state variants, and update strategy as decided during the v1 design pass. This is the doc a developer reads before starting launcher code, and the doc a future reviewer compares the final UI against.

Related docs:
- [`launcher-update-and-version-validation.md`](launcher-update-and-version-validation.md) — self-update mechanics, server-side version validation, HTTP 426 flow.
- [`../architecture/architecture.md`](../architecture/architecture.md) — package and module layout (`launcher/src/ui/`, `auth/`, `updater/`, etc.).
- [`../architecture/game-architecture-summary.md`](../architecture/game-architecture-summary.md) — identity system (Player ID + Secret Token), channel model.
- [`../dev/devtools.md`](../dev/devtools.md) — channel taxonomy (`stable` / `ea` / `dev`).

---

## 1. Layout

Five zones. Top bar, left rail, center pane, right rail, bottom bar.

```
+-----------+--------------------------------+--------------------------------+-------+
| v1.0.0    | Updates available: stable, ea  | Update available: launcher     | [gear]|
+-----------+--------------------------------+--------------------------------+-------+
| Channel   |                                                    | Username           |
| [stable▼] |              Briska Blast (title)                  | [Change Name]      |
+-----------+----------------------------------------------------+--------------------+
| Server    |                                                    | Player IDs        |
| status    |       Center pane                                  | Stable #0000007   |
|  ● stable |       (news / settings menus / placeholder)        | EA     #0000003   |
|  ● ea     |                                                    | (Dev hidden)      |
|  ○ dev    |                                                    |                   |
+-----------+----------------------------------------------------+--------------------+
| [Update]  | [==== blue: update in progress | === purple: remaining ====]   | [Play] |
+-----------+----------------------------------------------------+--------------------+
```

### Zone contents

| Zone | Contents |
|---|---|
| **Top-left** | Launcher version (`env!("CARGO_PKG_VERSION")`). Also surfaced in Settings later. |
| **Top-center** | Two update banners side-by-side. *Branch updates* (game files per channel) and *Launcher self-update* are separate code paths and stay visually distinct. Channels listed in the branch banner are filtered to those the user can see (see §3). |
| **Top-right** | Gear icon → Settings. |
| **Left rail (top)** | Channel selector. Conditional contents per §3. |
| **Left rail (bottom)** | Server-status panel. One dot per visible channel: `●` reachable, `○` unreachable (see §6). |
| **Center pane** | Selectable content. v1 default state: *"no menu selected"* placeholder. Becomes news feed when news system lands (`launcher/src/news/`). Settings opens as an overlay/panel within this zone. |
| **Right rail (top)** | Username + Change UserName affordance. **One** username per launcher install, shared across all channels (see §2). |
| **Right rail (middle)** | Per-channel Player IDs. Conditional rows (see §3). |
| **Bottom-left** | Update button (kicks off game-files update for the selected channel). |
| **Bottom-center** | Update progress indicator: blue segment = bytes/files in flight, purple segment = bytes/files remaining. |
| **Bottom-right** | Play button. Becomes "Running" with Update disabled while game is alive (see §7). |

---

## 2. Identity & Username File

One local JSON file owned by the launcher, stored in the platform's standard user-data directory (TBD per-OS path; see Open Items). Holds:

```jsonc
{
  "username": "BlastQueen99",
  "channels": {
    "stable": { "player_id": "0000007", "secret_token": "<hex>" },
    "ea":     { "player_id": "0000003", "secret_token": "<hex>" },
    "dev":    { "player_id": "0000042", "secret_token": "<hex>" }
  }
}
```

### Rules

- **Username** is a single shared field used as the display name on **every** channel. The "Change Name" affordance updates this one value. In-game peers see the same name regardless of which channel hosts the session.
- **Player IDs are per-channel**, sequential per server, canonical width 7 digits (e.g. `0000007`). A channel's entry is **absent** until that channel's server successfully issues an ID — see §3.
- **Secret tokens are per-channel** and never leave the file. They're used by the game's auth handshake, not displayed in the launcher UI. (Reaffirms `launcher-update-and-version-validation.md:139-167`.)
- The file is **launcher-owned**. The game receives `username` + the active channel's `player_id` (and secret) as startup parameters from the launcher, not from the server.
- On uninstall, prompt the user: *"Keep player data for future reinstall?"* (`launcher-update-and-version-validation.md:38-46`).

---

## 3. First-Launch Issuance & Channel Visibility Gating

### First-launcher-launch issuance

On the very first launcher launch (not first game launch), the launcher reaches out to **all three channel servers in parallel** to have an ID + secret_token issued for each. The results are saved to the identity file.

- **Per-server failure mode:** if a server is unreachable, that channel's entry **remains absent**. The other channels complete normally.
- **Retry semantics:** the launcher attempts issuance **once per launcher launch**. There is no infinite retry loop, no in-app retry button. Recovery for a previously-failed channel is: close the launcher and reopen it — the next launch retries the absent channels.

### Dev channel visibility

By default, **every** user has the dev row hidden — even users who successfully received a Dev player ID on first launch.

Visibility is gated by a **server-side per-user dev flag** on the dev server. An admin flips the flag for a specific user_id on the dev server's admin panel. The launcher learns the flag value on **every** launch as part of the dev-server handshake (the same call that refreshes channel-server connectivity). A newly-flagged user sees the dev surface appear on their next launcher launch.

The dev flag is the access right to **launch + download + install** the dev branch's resources and game files — not just UI visibility. Unflagged users cannot install dev files even if they wanted to.

### Visibility decision matrix

| Dev-server reachable? | Dev ID in file? | Dev flag from server? | Dev surface visible? |
|---|---|---|---|
| No | — | (unknowable) | **No** |
| Yes | No (never issued) | (irrelevant — no ID to attach to) | No |
| Yes | Yes | No | No |
| Yes | Yes | Yes | **Yes** |

The "dev-server unreachable" and "not dev-flagged" cases look identical in the UI (no dev row). The distinction surfaces only in the **Server status** panel of the left rail (see §6), so the dev team can see when their own dev server is down without leaking the existence of the channel to users.

### Update banner filtering

The "Updates available: ..." banner lists only channels the user can currently see. An unflagged user with stable + ea installed sees "Updates available: stable, ea" — never "stable, ea, dev". Leaking `dev` here would defeat the visibility gate.

---

## 4. Username Scope

| Field | Scope | Storage | Notes |
|---|---|---|---|
| `username` | **One** per launcher install | Local identity file (§2) | Used as in-game display name on every channel. Local-only — not pushed to channel servers. If server-side leaderboards / anti-cheat audit need it later, it can be sent at session-creation time as a request field. |
| `player_id` | One per channel | Local identity file (§2) | Per-server sequential, 7-digit. |
| `secret_token` | One per channel | Local identity file (§2) | Hashed server-side. Never displayed in launcher UI. |

---

## 5. State Variants

The canonical layout in §1 is the *steady, logged-in, unflagged-user* state. The launcher cycles through other states across its lifetime. For each, only the zones that **change** are described.

### A. First-launcher-launch in progress

Three concurrent reach-outs to issue IDs. Until they complete, the right-rail Player IDs zone shows a spinner or "Connecting…" indicator per row. The Play button is disabled. Server-status dots all show a transient "checking" state.

| Zone | What changes |
|---|---|
| Right rail (player IDs) | Each row shows `Connecting…` until that channel returns or times out |
| Left rail (server status) | Dots show a "checking" indicator (e.g., `◐`) |
| Bottom-right (Play) | Disabled |
| Bottom-left (Update) | Disabled |

No scary error modals on a failed reach-out — silent failure per §3. Failed channels resolve to absent rows (unflagged-user view).

### B. Partial-identity steady state

Example: stable + ea issued, dev server unreachable. User is unflagged for dev anyway. Looks identical to the standard unflagged-user view.

| Zone | What it shows |
|---|---|
| Right rail | Stable #0000007, EA #0000003. No Dev row. |
| Left rail (server status) | `● stable  ● ea  ○ dev` — the dev dot is `○` (unreachable). Dev team uses this to spot infra issues; users who don't have dev see this as just another channel that didn't come up. |
| Banner | "Updates available: stable, ea" (dev filtered out) |

### C. Steady state — unflagged user

Default view for a normal player. Matches the canonical layout in §1. Dev row hidden in right rail. Dev entry hidden in channel dropdown. Dev dot hidden from left-rail server-status panel.

### D. Steady state — dev-flagged user

Same as §C but with the dev surface visible: dev entry in channel dropdown, Dev player ID row in right rail, dev dot in server-status panel, dev included in updates-available banner.

### E. Game running

Triggered by Play. Launcher confirms no update is in flight for the active channel, then spawns the game.

| Zone | What changes |
|---|---|
| Bottom-right (Play) | Becomes `Running` indicator (non-button or disabled) |
| Bottom-left (Update) | Disabled with a tooltip explaining game is running |
| Top-center banners | Still visible, but Update button on each banner (if any) is disabled |
| Channel selector | Disabled (can't switch channels mid-game) |

### F. Update modal — game files for active channel

Triggered by Update button. Modal asks for confirmation, then progress is displayed in the bottom-bar progress indicator.

| Zone | What it shows |
|---|---|
| Modal | Release notes for the new game-files version, total download size, "Download and install?" yes/no |
| Bottom bar | Progress fills blue (in flight) → purple (remaining) |
| Play button | Disabled while update is in flight. **If Play is clicked, refuses with a message** — see §7. |

### G. Update modal — launcher self-update

Triggered by clicking the "Update available: launcher" banner. Uses the rename-trick flow via the `self_update` crate (`launcher-update-and-version-validation.md:54-73`).

Critical safety: refuse to start launcher update if the **game** is currently running (`launcher-update-and-version-validation.md:103-105`).

### H. Channel switch confirmation modal

Triggered by selecting a different channel in the dropdown. Switching channels typically means installing different game files.

| Field | Content |
|---|---|
| Title | "Switch to {channel}?" |
| Body | "Switching from {current} to {channel} requires downloading {size} of files. Continue?" |
| Buttons | Cancel / Switch |

May evolve later (e.g., once A/B install slots land per §7, channel switching becomes a free toggle between installed slots).

### I. HTTP 426 "Update required" prompt

Returned by a channel server when the launcher's `X-Launcher-Version` is below `min_launcher_version` (`launcher-update-and-version-validation.md:140-175`). Surfaces as a blocking modal: *"The {channel} server requires launcher version ≥ {min_version}. You have {current}. Update now?"* — leads into the launcher self-update flow (§G). Does **not** retry the failing request automatically.

---

## 6. Server Status Panel

Lives in the left rail, below the channel selector. One dot per visible channel.

| Symbol | Meaning |
|---|---|
| `●` | Channel server reachable on last launcher launch |
| `○` | Channel server was unreachable on last launcher launch |
| `◐` | Reachability check in flight (only during first-launch issuance, §5A) |

Dots refresh on launcher launch only (the same single-shot policy as ID issuance, §3). There is no live heartbeat. Hovering a dot may surface the last-checked timestamp and (for unreachable) the underlying error class (timeout / DNS / TLS / HTTP error) — useful in Settings or as a tooltip.

This panel is the only place where "dev server is unreachable" surfaces without exposing the dev channel concept to users — dev's dot is only shown to dev-flagged users (§3).

---

## 7. Update Model

### Two independent update streams

| Stream | What it updates | Trigger | Source |
|---|---|---|---|
| **Launcher self-update** | The launcher binary itself | "Update available: launcher" banner | GitHub Releases (via `self_update` crate) |
| **Game-files per-channel update** | The installed game's files for the active channel | "Update" button | Channel server / CDN (TBD: manifest format) |

These never overlap operationally — a launcher self-update replaces the launcher and relaunches; a game-files update modifies on-disk game assets without restarting the launcher.

### v1 placeholder rule: "refuse to launch during update"

If a **game-files update** is in flight for the active channel and the user clicks Play, the launcher **refuses to launch the game** until the update completes. The Play button is visibly disabled with a tooltip.

Acceptable as a v1 rule because the alternative (launching the old version while files are being rewritten) risks corrupting the in-progress install. The cost is a user who has to wait through a multi-GB download before playing again.

### Planned successor: A/B install slots

The eventual model is **A/B install slots**:

- Each channel has two install directories (slot A and slot B). Exactly one is "live."
- A game-files update downloads new files into the **inactive** slot while the user keeps playing the live slot.
- When the download completes and integrity verifies, the slot pointer atomically swaps. Old slot becomes the rollback target until the next update consumes it.
- The user never has to wait through a download to play — they keep playing the previous version, and the new version becomes active on the next game launch.

Implications when A/B lands:
- Channel switching becomes free if both slots happen to contain different channels (rare but possible).
- Rollback becomes trivial (swap the pointer back) — this addresses the deferred "Rollback mechanism" item in `launcher-update-and-version-validation.md:438`.
- The "refuse to launch during update" rule goes away.

Not in v1. Captured here so it doesn't get forgotten when the launcher's updater design matures.

### Compile-time launcher version

The launcher's own version is baked at compile time via Cargo:

```rust
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
```

Sent as the `X-Launcher-Version` HTTP header on every gated request. Server compares to its Redis-resident `min_launcher_version` and returns HTTP 426 if too old. Identical pattern to the server's `RELEASE_CHANNEL` bake (`server/build.rs:1-5`) and the client's `BuildConfig.Channel` bake (`client/BriskaBlast.csproj` GenerateBuildConfig target).

---

## 8. Open Items (Pre-Coding)

Decisions still to make before — or early into — launcher code:

- **Identity file location per OS.** Windows: `%APPDATA%\BriskaBlast\identity.json`? Linux: `$XDG_CONFIG_HOME/briskablast/identity.json`? Path conventions to be picked when the launcher `src/config/` module is built.
- **Manifest format for game-files updates.** What does the channel server hand the launcher to enumerate "what files exist in this version, what hashes do they have"? Out of scope for the UI, but blocks the actual update implementation.
- **Channel-server endpoint shape for the launcher's per-launch handshake.** Currently the server exposes `POST /register` for first-time identity issuance (per `architecture.md:32`). The dev-flag check and the connectivity check need a target endpoint — possibly an extension of `/register` to return `{player_id, secret_token, dev_flag}` and accept "already registered" responses idempotently. To be designed alongside the launcher's `src/auth/` module.
- **Username uniqueness scope.** Is "BlastQueen99" globally unique, per-channel unique, or non-unique (just a display label)? Likely non-unique for v1 (local-only, no server-side checks), but worth confirming before showing the change-name UI.
- **Server-status tooltip surface.** Hover details on the left-rail dots — implement now or defer to a Settings → diagnostics panel?

---

## 9. Explicitly Deferred (Don't Build Now)

Captured here so future contributors don't re-litigate.

- **Cryptographic binary signing** — `launcher-update-and-version-validation.md:438`.
- **Delta updates (bsdiff / zstd)** — same doc.
- **Bootstrap stub launcher** — only revisit if the rename-trick causes friction.
- **Steam/Epic integration** — would replace the identity system + Layer 1 installer entirely.
- **WS-ticket auth** — `../planning/roadmap.md:32-37`. Launcher will eventually issue short-lived signed tickets so `secret_token` never crosses the WebSocket; deferred until Godot client + launcher land together.
- **TURN credentials delivery** — `../planning/roadmap.md:25-30`. Launcher will be the place credentials are issued from once TURN exists.
- **Proactive notifications when dev flag flips** — current model is pull-based; a newly-flagged user sees the dev surface on their next launch. Acceptable; no push needed.
- **A/B install slots** — captured in §7. Targeted post-v1.
- **Live heartbeat on server-status dots** — single-shot per launch is fine until it isn't.
