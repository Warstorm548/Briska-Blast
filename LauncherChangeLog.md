# Launcher Changelog

All notable changes to the Briska Blast launcher are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [0.14.2] — 2026-06-17

Closes a footgun in the game install flow: the user could pick an install folder
that sat **inside the launcher's own application directory**, nesting the game
under the launcher. Such an install is broken — the game won't launch and never
gets its Windows firewall rule prompt. The launcher now refuses these locations.

### Fixed

- **Reject install locations that collide with the launcher's own folder**
  (`paths::install_location_collides`, wired into `app/handlers/install.rs`). The
  resolved game install dir (`<chosen folder>/<channel>/`) is compared against the
  launcher's install directory after canonicalizing both; if either is nested in
  the other (or they're equal), the install is refused with a blocking message in
  the install prompt rather than producing a game that can't launch or obtain its
  firewall permission. Enforced at **both** the folder-picker step (immediate
  feedback, Confirm stays disabled) and at confirm time (defence-in-depth, also
  covering an update whose path comes from `identity.json`). Applies on all
  platforms. Legitimate siblings (e.g. launcher `…/BriskaBlast`, game `…/dev`) are
  unaffected — the check is component-wise, not a string prefix.

---

## [0.14.1] — 2026-06-16

A small footprint reduction on update discovery. The first Part 4 lever from the
rate-limit design doc; fetch-once and ETag remain deferred.

### Changed

- **Release discovery now requests `per_page=100`** (`updater/github_client.rs`),
  GitHub's maximum, instead of the default 30. Each update check pulls the full
  release list in **one** page rather than paginating, so at the current ~46
  releases a check drops from **2 GitHub requests to 1** — roughly halving the
  per-launch API footprint (a returning user's boot fan-out goes from ~6 to ~3).
  The `Link: rel="next"` pagination loop is retained, so a repo that ever exceeds
  100 releases still fetches correctly. No behavioural change to what's discovered.

---

## [0.14.0] — 2026-06-15

Adds a **GitHub rate-limit back-off safety net**. The launcher discovers updates
against GitHub Releases unauthenticated, so every user shares GitHub's 60-req/hr
**per-IP** bucket. Previously the launcher never read the rate-limit signal and so
kept knocking once throttled — exactly the "ignoring the back-off" pattern that can
escalate a harmless hourly throttle into GitHub's secondary limits or an IP block.
Now, once a rate-limit is observed, the launcher goes quiet until the window resets.
This is back-off only; it does **not** reduce request volume (that footprint work —
`per_page=100`, ETag, fetch-once — stays deferred, see
`docs/planning/launcher-github-ratelimit-safety-net.md` Part 4).

### Added

- **`ratelimits.json`** in the per-user data dir, next to `identity.json`
  (`ratelimit.rs`, `paths::ratelimit_path`). Stores GitHub's last-seen
  `X-RateLimit-Reset` + `X-RateLimit-Remaining` and a derived resume instant, so the
  gate is a trivial local file read — never itself a GitHub request.
  - **Reset gate (Layer A):** on a confirmed `403`/`429` rate-limit, block all
    GitHub-counting requests until `reset` + a 2-minute clock-skew pad.
  - **Proactive stop (Layer B):** read `X-RateLimit-Remaining` off every response;
    go quiet at `remaining ≤ 5` until the same `reset` + 2 min.
  - **No-header fallback:** a confirmed rate-limit lacking a reset header → fixed
    1-hour cooldown (near-dead path on public GitHub).
  - **Fails OPEN:** a missing/corrupt `ratelimits.json` never bricks update checks.
- **A clear, gated state surfaces on the manual checks.** The left-rail per-channel
  *Updates* box reads *⏳ GitHub limit — retry at HH:MM*
  (`ChannelUpdateStatus::RateLimited`), and the launcher self-update check reports
  the same in Launcher Options. Both pre-check the gate synchronously, so a blocked
  press spends no request and shows the resume time at once.

### Changed

- **Update discovery now owns its GitHub request** (`updater/github_client.rs`).
  Both the launcher self-update check and the per-channel game `latest_release`
  discovery moved off `self_update`'s `ReleaseList::fetch()` (which hides the HTTP
  status + headers) onto a direct `reqwest` call that exposes the status and the two
  rate-limit headers — the prerequisite for the safety net. The request footprint is
  unchanged (still paginates 30/page following `Link: rel="next"`); `self_update`
  still powers the launcher binary self-update swap and stale-artifact cleanup.
- **The gate covers every counted GitHub request:** the launcher self-update check,
  the per-channel game `latest_release` checks, and the binary-download asset
  request (`updater/branches/installer.rs`). A user-initiated install that's
  rate-limited fails fast with a clean "resumes at HH:MM" rather than starting and
  dying mid-flight on a 403. Actions that hit our own server (register / reachability)
  or stay local (uninstall / verify / saves / firewall) are untouched.
  - **Correctness guardrail:** the cooldown arms only on a *confirmed* `403`/`429`
    rate-limit (`X-RateLimit-Remaining: 0` or a `Retry-After`), **never** on a
    generic network error/timeout — a Wi-Fi blip cannot cause a lockout.

---

## [0.13.1] — 2026-06-15

Relocates the manual update check from Settings to the **left rail**, directly
under the channel picker — a single button scoped to the focused channel,
replacing the per-channel **Channel Updates** table in Settings (0.13.0). The
check logic is unchanged; this is a UI move plus a clear-on-switch refinement.

### Changed

- **"Check for Updates" now lives under the channel selector** (`ui/left_rail.rs`).
  One button checks only the channel currently selected in the dropdown, and the
  verdict renders in a bordered box directly below it, labelled *Updates · <Channel>*:
  *● Update available — vX.Y.Z*, *✓ You're up to date — vX.Y.Z*, *⚠ Check failed*,
  or *Checking…* (the button itself also shows *Checking…* and is disabled) while
  a fetch is in flight. The box reads the em-dash until the channel is checked.
  - A found update still flips the bottom-bar Update button — it reads the same
    `available_versions` cache the check refreshes — so clicking that button is
    how the user proceeds with the update. No separate update path was added.
  - The button stays disabled until the focused channel is installed and no
    install / uninstall / running-game is in progress, matching the old row.
- **The verdict box clears when the focused channel changes** (`app/handlers/nav.rs`).
  Switching the dropdown drops completed verdicts so the box resets, but keeps
  any in-flight `Checking` sentinel (so a running check isn't dropped, its button
  stays deduped, and "Checking…" returns if the user switches back mid-flight).
  `available_versions` is left intact, so the bottom-bar button keeps its
  per-channel state.

### Removed

- The **Channel Updates** subsection in Settings → Game Channel Management and its
  per-channel status cells (`ui/center/settings.rs`), superseded by the left-rail
  button above. The launcher self-update check in Settings → Launcher Options is
  unaffected.

---

## [0.13.0] — 2026-06-14

Adds a **manual per-channel update check** so a release published while the
launcher is open can be picked up without a restart (the GitHub `latest_release`
fan-out otherwise only runs once at boot).

### Added

- **"Check for Updates" button per channel** under a new **Channel Updates**
  subheading in Settings → Game Channel Management (`ui/center/settings.rs`,
  `app/handlers/install.rs`, `app/state.rs`). Pressing it re-queries GitHub for
  the channel's latest `game-v*` release and reports the verdict in an inline
  status box: *Update available — vX.Y.Z*, *You are up to date — vX.Y.Z*,
  *Checking…* while in flight, or *? Check failed* on a fetch error.
  - The check writes only to the existing `available_versions` cache and reuses
    `recompute_branch_updates_available`, so the bottom-left Update/Install
    button keeps its exact state-machine — it flips to *Update to vX.Y.Z* for the
    channel currently selected in the dropdown. Channels are checked one at a
    time (no auto-switch, no batch update) as a deliberate fail-safe while
    multi-channel update orchestration is still unbuilt.
  - The Dev row is gated behind the server-assigned dev flag (`visible_channels`),
    matching every other section; the handler keeps a defence-in-depth
    `Dev && !dev_flag` guard.
  - A successful install clears the channel's status box so it can't keep
    claiming an update is available for a channel just updated.

---

## [0.12.1] — 2026-06-07

Internal refactor only — **no behavior change**.

### Changed

- **`app.rs` split into an `app/` module tree** for maintainability (it had grown
  to ~1487 lines). The single file is now `app/mod.rs` (the `update` dispatcher,
  shared helpers, `boot`/`view`/`theme`/`title`), `app/message.rs` (`Message`,
  `CenterView`, `SettingsTab`), `app/state.rs` (`AppState`), and per-feature
  handlers under `app/handlers/` (`nav`, `launcher_update`, `identity`, `install`,
  `play`, `maintenance`, `firewall`). Each `update` match arm moved verbatim into a
  handler function; the public `crate::app::{…}` surface and runtime behavior are
  unchanged (build, clippy on the Linux + `x86_64-pc-windows-gnu` targets, and the
  test suite all pass).

---

## [0.12.0] — 2026-06-05

Identity **self-heal** when a channel's server-side id is deleted. Pairs with
server **v0.12.0**, which adds admin user-deletion + id reuse — a deleted player's
stored creds stop validating, and the launcher now recovers cleanly instead of
silently dropping the action.

### Changed

- **401 → re-register on username change** (`server_api.rs`, `app.rs`).
  `update_username` now distinguishes HTTP `401` from other failures via a new
  `ServerApiError { Unauthorized, Other }`. On `Unauthorized`, the
  `UpdateUsernameDone` handler re-dispatches `/register` for that channel: the
  server rejects the stale creds, issues a fresh id (recycled from the pool), and
  the existing `RegisterDone(Ok)` path persists it and re-applies the username —
  one round-trip fully heals the channel. (Boot-time re-register already covered
  the next-launch case; this closes the live-session gap.)

---

## [0.11.0] — 2026-05-26

Extends the game handoff so the launched client can authenticate to the
server. Part of Stage 1 of multiplayer — see
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).

### Changed

- **Identity + version handoff.** The launcher→game handoff file grew from
  `{username}` to `{username, player_id, secret_token, launcher_version,
  channel}` (`game_launch/mod.rs`). The client needs the channel's
  `player_id` + `secret_token` to authenticate every server call
  (`/host`, `/join`, the WS `identify`), and `launcher_version` to satisfy
  the version gate's `X-Launcher-Version` check — neither of which the game
  can know on its own. Creds are pulled from the channel's `ChannelCreds`
  at the single `launch_game` choke point in `app.rs`.

### Security

- The handoff file now carries the `secret_token`. On unix it is already
  created `0o600`; the Windows plaintext-in-temp exposure is a known v1 gap
  (uuid-named, short-lived). The WS-ticket auth roadmap item removes the
  raw token from the wire entirely.

## [0.10.0] — 2026-05-25

macOS (Apple Silicon) support for the launcher itself — Stage A of the macOS
effort. The launcher now builds, ad-hoc signs, and packages for
`aarch64-apple-darwin`, so it runs on an Apple Silicon Mac.

> Note: this jumps 0.8.2 → 0.10.0 because the firewall-elevation work (0.9.0,
> below) merged just before it.

### Added

- **macOS build + packaging (Apple Silicon / `aarch64-apple-darwin`).** Release
  workflow now has a `build-macos` job (on `macos-latest`, native arm64) that
  publishes the `self_update` `…-aarch64-apple-darwin.tar.gz` plus an
  ad-hoc-signed `.app` inside a `.dmg`, via `.github/scripts/make-macos-app.sh`
  (Info.plist + `.icns` from `icon.png` + `codesign --sign -` + `hdiutil`).
- **macOS CI.** `ci-launcher.yml` now runs fmt/clippy/build/test on `macos-latest`.
- **macOS game-install path.** `installer.rs` now selects the `macos.tar.gz`
  game asset on macOS and resolves the in-bundle Mach-O
  (`BriskaBlast.app/Contents/MacOS/BriskaBlast`) as the manifest executable, so
  the launcher can install and launch the macOS game build. (The matching Godot
  macOS export ships in the game's v0.3.0 — see `GameChangeLog.md`.)

### Notes

- Ad-hoc signing is tester-grade: the app runs locally past Gatekeeper for a
  known tester (right-click → Open the first time, or strip the quarantine
  xattr), but is **not** Developer-ID signed or notarized for public download.
- No launcher code change was needed for self-update on macOS — `self_update`
  auto-detects the compile-time target triple and matches the macOS tar.gz asset.
  Per-user data already resolves to `~/Library/Application Support/BriskaBlast/`
  via the `directories` crate.

---

## [0.9.0] — 2026-05-25

Completes the Windows-firewall story started in the Layer-1 hardening work
(PR #44, which shipped read-only detection). This release adds the **elevated
write** — option A: user-initiated, single UAC elevation.

### Added

- **First-Play firewall prompt (Windows).** When the user hits Play for a
  channel whose game has no inbound firewall rule, the launcher runs a
  non-elevated check and, if the rule is missing, shows a one-time prompt:
  **Allow & Play** creates the rule via a single UAC elevation, **Skip & Play**
  launches without one (suppressed for that channel for the session). Hosting a
  match needs the rule because the game uses WebRTC/NAT hole-punching and the
  game exe is downloaded per-channel at runtime (so its path isn't known at
  install time — only the launcher can create the rule).
- `firewall::add_inbound_rule_elevated` now runs, elevated, the equivalent of
  `netsh advfirewall firewall add rule name="BriskaBlast <channel> Game"
  dir=in action=allow program="<exe>" enable=yes`. Elevation uses the `runas`
  crate (Windows-only dependency, `windows-sys`-based: ShellExecuteExW with the
  `runas` verb → single UAC → waits → exit code). Args are passed discretely and
  quoted/escaped by `runas`; the rule name derives from the fixed `Channel`
  enum, so there is no command-injection surface.

### Notes

- On Linux/macOS this is a no-op: outbound-initiated hole-punching traverses the
  default host firewall, so there is nothing to add. `add_inbound_rule_elevated`
  is a Windows-only path; the non-Windows stub returns `Err`.
- The skip dismissal is in-memory (re-prompts on next launcher launch while the
  rule is still missing). A persisted opt-out, plus a Settings-panel button as a
  second entry point, are tracked in the roadmap.

---

## [0.8.2] — 2026-05-24

Root-cause hotfix for the v0.8.0 / v0.8.1 install failure
`install failed: zip open: invalid archive: Could not find EOCD`.
v0.8.1's Content-Length checks did not fire because the underlying
problem wasn't a truncated download — it was a **wrong-content
download**.

### Diagnosis (three-way confirmed)

`self_update::backends::github::ReleaseList` parses `asset["url"]` (the
GitHub REST API endpoint, NOT `browser_download_url`) into
`ReleaseAsset.download_url`. Our `launcher/src/updater/branches/
github.rs:78` forwards that field straight into our wrapper, so we
**were** hitting the right URL — but the GitHub API returns an asset's
JSON metadata (~few hundred bytes) when called without an
`Accept: application/octet-stream` header. We saved that JSON as
`.download-foo.zip`, the extractor unsurprisingly couldn't find an
EOCD record, the user got the cryptic message.

`self_update`'s own download path (`self_update-0.41.0/src/update.rs:234`)
sets exactly this header, which is why the launcher's **binary
self-update** path has worked all along — only our hand-rolled
**game-files** download was missing it.

Verified by an online research agent (citing
`https://docs.github.com/en/rest/releases/assets`), a cargo-registry
source-code read of self_update, and re-reading our own code. All three
agreed on root cause + fix.

### Fixed

- **`Accept: application/octet-stream` header** is now set on the GET
  in `stage_install` (`launcher/src/updater/branches/installer.rs`).
  This is the single line that fixes the user-reported bug.
- **Magic-byte sniff** added post-download. Confirms the file on disk
  starts with `PK\x03\x04` (zip) or `1f 8b` (gzip) before the extractor
  ever opens it. On mismatch, the error includes the first 4 bytes and
  a 256-byte sample of the file content — so the next failure (if any)
  carries its own diagnosis (`{"url":...}` ⇒ JSON metadata regression,
  `<html>` ⇒ CDN error page, etc.) without needing more telemetry.
- **Logging level**: the pre-extract `downloaded / total / on_disk`
  trace is now `tracing::info!` instead of `tracing::debug!` so users
  running from a terminal see byte counts without env-filter changes.

### Removed

- v0.8.1's `if total > 0 && downloaded != total` Content-Length-based
  truncation check and the `on_disk != total` cross-check. Both were
  superseded by the magic-byte sniff (which covers both truncated AND
  wrong-content cases without depending on Content-Length being set).
  Their tracing::debug log moved up to info-level.

### No new deps; no schema changes.

---

## [0.8.1] — 2026-05-24

Hotfix for a v0.8.0-reported install failure surfacing as
`install failed: zip open: invalid archive: Could not find EOCD` on
Windows when installing the dev game. The underlying cause is a
silently-truncated download — `reqwest::Response::bytes_stream`
finishes without an error when the server closes the connection
mid-stream, leaving a short file that the zip extractor (correctly)
refuses to parse with the unhelpful EOCD message. The fix:

### Fixed

- **Truncated-download detection** (`launcher/src/updater/branches/
  installer.rs`). After the streaming download loop the launcher now
  asserts `downloaded == Content-Length` and bails with a clear
  message ("download truncated: wrote X bytes, expected Y — connection
  likely dropped mid-stream; retry") instead of letting the broken
  byte stream tumble into the extractor. A second check stats the file
  on disk and compares to `total` so a file-handle race (download
  bytes lost between user-space and the kernel) surfaces with its own
  distinct message.
- **Explicit file close before extract**. Replaced `drop(file)` with
  `file.sync_all().await + file.shutdown().await + drop(file)`. The
  bare drop doesn't reliably finish the underlying close on Windows
  before `spawn_blocking` re-opens the same path for reading, which
  can race the extractor against a partial flush.
- **Diagnostic logging**. Added a `tracing::debug!` line emitting
  downloaded / content-length / on-disk byte counts before extraction
  so the next failure (if any) tells us exactly what shape it took
  without requiring code edits.

### Action for affected users

If you saw the EOCD failure on v0.8.0, re-install after self-updating
to v0.8.1. Half-finished installs in `<install_root>/.<channel>.staging-*`
sibling dirs left by v0.8.0 can be deleted by hand or will be cleaned
on the next install attempt via Stage 5's atomic-swap path.

---

## [0.8.0] — 2026-05-24

Wires the three previously-dead Settings → Game Channel Management
buttons (Uninstall, Verify File Integrity, Game Save) to real per-channel
actions. The launcher's per-channel lifecycle is now complete: install
→ update → play → verify → uninstall.

Stage 7 of the launcher game-install pipeline plan. Final stage before
the feat-branch → dev merge.

### Added

- **Uninstall flow** with a confirmation prompt
  (`launcher/src/ui/center/uninstall_confirm.rs`, new
  `CenterView::UninstallConfirm` route). Prompt shows the channel,
  installed version, resolved install dir, and a "Keep player data for
  future reinstall?" Yes/No radio (foundation §2). On confirm:
  - if Keep, `<install_dir>/saves/` is moved to a **timestamped**
    sibling at `<install_root>/.briska-saves-backup/<channel>/<rfc3339>/`
    so a re-install / re-uninstall cycle never overwrites prior saves;
  - then `<install_dir>` is removed wholesale;
  - then `install_location` and `installed_version` are cleared from the
    channel row of `identity.json`, and `state.verify_results` for that
    channel is dropped.
  Uninstall is intentionally allowed regardless of `state.dev_flag` so a
  previously-flagged user who got revoked can still clean up orphan dev
  files. Errors keep the prompt open with an inline status line.
- **Verify File Integrity flow** — cheap variant per the Stage 7
  decision: re-reads `<install>/installed.json`, confirms the manifest's
  named executable still exists on disk. Outcome cached in
  `state.verify_results: BTreeMap<Channel, VerifyOutcome>` and surfaced
  inline in the channel row as `— / ✓ Verified vX.Y.Z / ✗ Manifest
  missing / ✗ Manifest unreadable / ✗ Executable missing`. Per-file
  hashing + saves-dir-intact mode were captured in
  `docs/planning/roadmap.md` as deferred options.
- **Game Save button** opens the channel's `<install>/saves/` directory
  in the OS file manager (new `open = "5"` dep, cross-platform xdg-open
  / explorer / Finder). Saves dir is created on demand if it doesn't
  exist yet.
- **`updater::branches` primitives**: `uninstall_install(install_dir,
  channel_dir_name, keep_saves)`, `verify_install(install_dir)`,
  `VerifyOutcome` enum, `SAVES_BACKUP_DIRNAME` constant.

### Changed

- **`UninstallChannel(Channel)`, `VerifyChannel(Channel)`,
  `GameSavePressed(Channel)`** lose their `#[allow(dead_code)]` — all
  three are now wired end-to-end.
- **Settings → Game Channel Management** rows disable their
  Uninstall / Verify buttons when the channel has no install on record
  (and during `install_in_progress` / `game_running`). The Game Save
  button in the "Game Important Files" section follows the same rule.
- **Settings rows widen** to include the new verify status cell next to
  Verify File Integrity.

### Dev gating

- `VerifyChannel` and `GameSavePressed` defence-in-depth refuse
  `Channel::Dev` when `state.dev_flag` is false. `UninstallChannel`
  intentionally does **not** — orphan-cleanup is allowed for revoked
  users.

### Deps

- New: `open = "5"` for cross-platform file-manager launch.

### Roadmap additions

`docs/planning/roadmap.md` gained:
- **Per-file hash manifest + deep Verify File Integrity** (re-hash every
  file at install time, compare on Verify).
- **Saves-dir intact verify mode** (alt cheap variant; pairs with the
  future saves-relocation work).

---

## [0.7.1] — 2026-05-24

Bottom progress bar wired through to real install events. The
`download_and_install` callback that fed `tracing::debug` since Stage 3
now feeds an `iced::widget::progress_bar` plus a human-readable label
("Downloading — 64% (122 MiB / 192 MiB)" / "Extracting…" / "Done."). The
last mocked piece of the launcher UI — `MOCK_PROGRESS_PERCENT` — is gone;
`mock.rs` is deleted, no module remains to maintain.

Stage 6 (and final code stage before Stage 7's uninstall wiring + the
feat-branch → dev merge) of the launcher game-install pipeline plan.

### Added

- **`AppState.download_progress: Option<InstallProgress>`**
  (`launcher/src/app.rs`) — last progress event from the active install
  pipeline. `None` between installs.
- **`Message::DownloadProgress { channel, progress }`** — per-chunk
  download/extract event, mapped 1:1 from the internal
  `InstallStreamEvent::Progress` variant. Stale-channel events (i.e.
  events for a channel that's no longer `install_in_progress`) are
  discarded by the handler.
- **Streaming `InstallConfirmed` task** — the install pipeline now drives
  an `iced::Task::stream` over a tokio `UnboundedReceiver`, so the
  bottom-bar widget updates in real time during the download instead of
  jumping straight from idle to complete on InstallComplete. The single
  `tokio::spawn` owns the work and writes Progress / Complete events
  into the channel; the receiver is adapted via
  `tokio_stream::wrappers::UnboundedReceiverStream` and the events are
  `.map`-ped to the matching public Message variants via
  `futures_util::StreamExt`.

### Changed

- **`InstallProgress`** (`launcher/src/updater/branches/installer.rs`)
  fields are no longer `#[allow(dead_code)]` — `bytes_now`,
  `bytes_total`, and `fraction` are now consumed by the new
  `bottom_bar::progress_cell`.
- **`bottom_bar::progress_cell`** renders an `iced::widget::progress_bar`
  plus a label line. `format_bytes` helper formats bytes/KiB/MiB/GiB
  appropriately for the human-readable counter.

### Removed

- **`launcher/src/mock.rs`** — deleted along with `MOCK_PROGRESS_PERCENT`
  and the comments explaining the prior mocks. `mod mock` dropped from
  `main.rs`. No mocked UI state remains.

### Deps

- New: `tokio-stream = "0.1"` for `UnboundedReceiverStream`.

### Deferred to Stage 7

- Per-channel **Uninstall** + **Verify** + **Game Save** buttons in the
  Settings → Game Channel Management tab. `Message::UninstallChannel`,
  `Message::VerifyChannel`, and `Message::GameSavePressed` variants
  exist as `#[allow(dead_code)]` no-ops; Stage 7 wires the destructive
  uninstall (confirm modal → `tokio::fs::remove_dir_all` → clear
  `install_location` / `installed_version` on the identity row), the
  verify path (re-read `installed.json`, sanity-check the executable
  exists), and the saves-folder open action. Captured in the plan's
  staging table; needed before the final feat-branch → dev push.

---

## [0.7.0] — 2026-05-24

Play button now actually launches the installed game. With an installed
channel selected and no install in flight, pressing Play writes a
one-shot handoff JSON to the OS temp dir (`{"username": "..."}`, perms
`0600` on unix), spawns the channel's game executable with
`--launcher-handoff <path>`, and awaits exit. The game reads + deletes
the handoff file on startup (Stage 1's `LaunchArgs.FromLauncher`),
returning the username to `SessionContext.LocalUsername` before any UI
renders. While the game is running the bottom-right button reads
`Running`, the bottom-left button is disabled with `Running`, and the
channel selector collapses to a static label so a mid-session channel
switch can't race the spawned process.

Stage 5 of the launcher game-install pipeline plan. Only the bottom
progress bar's mock-percent display remains for Stage 6.

### Added

- **`launcher/src/game_launch/`** (new module): `spawn_and_wait(channel,
  install_dir, username)` reads the per-install `installed.json` to find
  the executable, writes a uuid-named handoff file under `std::env::
  temp_dir()`, spawns the binary with `tokio::process::Command` so the
  wait is awaitable, returns the exit code (or `None` for signal exits).
  Cleans up the handoff file on exit in case the game never read it.
  Unit-tested for handoff JSON round-trip + path uniqueness.
- **`Message::GameExited { channel, result }`** (`launcher/src/app.rs`)
  fires when the spawned game process exits; clears `state.game_running`.
- **`PlayPressed` handler** (`launcher/src/app.rs`) — was a no-op through
  0.6.x. Now validates `(installed_version present, install_location
  present, no install in flight, not Dev-without-flag)`, sets
  `game_running = true`, and dispatches the spawn task. The half-state
  guard from Stage 3 is the gate — `ChannelCreds::parsed_installed_version`
  must return `Some(_)` for Play to proceed.

### Changed

- **`updater::branches::installed_manifest`** is now publicly re-exported
  (was `#[allow(dead_code)]` in Stage 3) — game_launch reads it on every
  Play to resolve the executable's relative path inside the install dir.
- **Channel picker** (`launcher/src/ui/left_rail.rs`) collapses to a
  static `text` label while `game_running` is true. Re-renders as the
  interactive `pick_list` as soon as `GameExited` clears the flag
  (foundation §5E).
- **Cargo.toml**: `tokio` feature flags gain `"process"` (async
  `Command::spawn` + `.wait()`); new `uuid = { version = "1", features
  = ["v4"] }` for unique handoff filenames.

### Dev gating

- `PlayPressed` refuses `Channel::Dev` when `state.dev_flag` is false
  (defence-in-depth — the channel selector hides Dev under that
  condition).

### Handoff protocol invariants

- Filename: `${TMPDIR}/briskablast-handoff-<uuid>.json` (v4 random uuid).
- Perms: `0600` on unix; best-effort (failure logged, not fatal).
- Lifecycle: launcher writes pre-spawn; game reads + deletes on
  startup; launcher removes any leftover on game exit.
- Payload schema is stable per `client/src/core/LaunchArgs.cs:Handoff`
  — only `username` for now, with room to add `player_id`,
  `secret_token`, `server_url` later (roadmap items).

### Deferred to Stage 6

- The `progress_cell` in `bottom_bar.rs` still shows the
  `MOCK_PROGRESS_PERCENT` placeholder. Stage 6 wires the real
  `InstallProgress` events (already emitted by `download_and_install`)
  through to an `iced::widget::progress_bar`.

---

## [0.6.0] — 2026-05-24

Per-channel version detection on boot, plus the full bottom-left button
state machine. The launcher now queries GitHub Releases for the latest
`game-v*` tag of every visible channel on launch, caches the result in
`state.available_versions`, and derives both the bottom-left button label
and the top-bar "Updates available" banner from real installed-vs-available
comparisons. The Stage 3 stub label `Update` is gone; the button now cycles
through `Install <Channel> Game` / `Update to vX.Y.Z` / `Up to date —
vX.Y.Z` / `Installing…` / `Running` per the foundation table.

Stage 4 of the launcher game-install pipeline plan. Stage 5 wires Play;
Stage 6 polishes the progress bar.

### Added

- **`AppState::available_versions: BTreeMap<Channel, semver::Version>`**
  (`launcher/src/app.rs`) — populated by per-launch fan-out of
  `updater::branches::latest_release(channel)` for every visible
  channel. Dev's fetch fires only when the dev `/register` response
  returns `dev_flag = true`, so unflagged users never reach the GitHub
  API for the dev channel (foundation §3 defence-in-depth).
- **`recompute_branch_updates_available`** (`launcher/src/app.rs`) —
  derives `state.branch_updates_available` from real `(installed,
  available)` pairs, filtered to `visible_channels` so the dev channel
  never leaks into the top-bar banner for an unflagged user. Called
  from every handler that mutates available / installed / visible
  state.
- **`Message::LatestReleaseFetched { channel, result }`** — the
  per-channel release-discovery completion. Late-arriving Dev fetches
  after a dev_flag revoke are dropped at the handler so the cache
  can't be poisoned.
- **`ChannelCreds::parsed_installed_version`** (`launcher/src/identity.rs`)
  — returns the parsed `semver::Version` only when both
  `install_location` and `installed_version` are present, so the
  half-state guard from the Stage 3 review is the single source of
  truth for "is this channel actually installed?" across `bottom_bar`
  and `recompute_branch_updates_available`.

### Changed

- **Bottom-left button** (`launcher/src/ui/bottom_bar.rs`) is now fully
  state-driven from `(installed_version, available_versions[channel],
  game_running, install_in_progress)`. Labels cycle: `Install <C> Game`
  (enabled iff a release exists), `Update to vX.Y.Z` (enabled iff
  newer), `Up to date — vX.Y.Z` (disabled), `Installing…` (disabled),
  `Running` (disabled).
- **Top-bar banner** (`launcher/src/ui/top_bar.rs`) consumes the same
  derived list; the previous `mock::BRANCH_UPDATES_AVAILABLE` constant
  is retired (replaced with an explanatory note in `mock.rs`).
- **`Message::UpdatePressed` handler** (`launcher/src/app.rs`) handles
  both the install-fresh and update-outdated routes. The install prompt
  is opened with `available` pre-populated from the boot cache (no
  re-fetch when the cache is warm); for the update path,
  `install_root` is pre-filled from the existing install_location so
  the user isn't asked to re-pick the folder.
- **`RegisterDone(Dev)` handler** now spawns the Dev `latest_release`
  task when `dev_flag` flips true, and drops any cached Dev version
  when it flips false or the dev server is unreachable. Keeps
  `available_versions` in lockstep with the visibility gate.

### Boot order

The fan-out shape on a launch with an existing username:
1. `updater::check_for_update` (launcher self-update, unchanged).
2. `register_tasks` — one /register per `Channel::all()`.
3. `latest_release_tasks` — one GitHub query per *visible* channel
   (Stable + Ea up front; Dev follows RegisterDone(Dev, Ok)).

First-launch users follow the welcome flow; the same fan-out runs after
`ConfirmWelcomeUsername` persists the chosen name.

### Deferred to later stages

- **Stage 5** — Play button spawn (with the Stage 1 `--launcher-handoff`
  temp-file).
- **Stage 6** — Bottom progress bar reads the existing
  `InstallProgress::Downloading { fraction, bytes_now, bytes_total }`
  events (already emitted, currently only traced).

---

## [0.5.0] — 2026-05-23

Per-channel game-files install pipeline. The bottom-left button is now
channel-aware: when the selected channel has no game installed, it reads
`Install <Channel> Game` and routes to a new center-pane install prompt
that lets the user pick a folder and confirm. On confirm, the launcher
queries GitHub Releases (filtered by `game-v*` tags + channel suffix),
downloads the platform-appropriate artifact, extracts it into
`<chosen_root>/<channel>/`, and writes an `installed.json` manifest plus
new `install_location` / `installed_version` fields on the channel's row
of `identity.json`. Dev channel is fully gated behind `dev_flag` — the
launcher refuses to query GitHub, open the install prompt, or run the
download for `Channel::Dev` unless the cached dev_flag is true.

Stage 3 of the launcher game-install pipeline plan. Stage 4 wires the
"installed-but-outdated" detection on top; Stage 5 wires Play; Stage 6
polishes the progress bar.

### Added

- **`launcher/src/updater/branches/`** (new module — replaces the
  `.gitkeep` placeholder reserved in foundation §8):
  - `github::latest_release(channel)` — lists GitHub Releases via
    `self_update::backends::github::ReleaseList`, filters by `game-v`
    prefix and anchored channel suffix (mirrors `release-client.yml`'s
    regex). Returns the highest-semver `GameRelease`, or `None` if no
    matching release is published yet. Unit-tested for stable / ea / dev
    parsing.
  - `installer::download_and_install` — picks the platform asset
    (`linux.tar.gz` / `windows.zip` substring match), streams the
    download with `reqwest`'s `bytes_stream` + `Content-Length` for
    fractional progress, extracts via `tar`+`flate2` (linux) or `zip`
    (windows), writes `<install_dir>/installed.json` with version,
    channel, RFC3339 install timestamp, and the resolved executable
    relative path. Progress is reported through an `InstallProgress`
    callback now wired to `tracing::debug` (Stage 6 will route the
    fractional bytes to the bottom progress bar).
- **`ChannelCreds` extended** with two new optional fields,
  `install_location: Option<PathBuf>` and `installed_version:
  Option<String>` (`launcher/src/identity.rs`). `#[serde(default)]` on
  both keeps pre-0.5.0 identity files forward-compatible. New helper
  `ChannelCreds::from_register` builds a row from a `/register`
  response, and the `RegisterDone` handler now preserves any prior
  install state so a routine register refresh does not wipe the
  channel's install record.
- **`CenterView::InstallPrompt`** (`launcher/src/ui/center/install_prompt.rs`,
  `launcher/src/app.rs`) — new center-pane route showing channel, latest
  available version, an OS-native folder picker (`rfd::AsyncFileDialog`),
  and Confirm / Cancel buttons. Confirm is disabled until both a folder
  and a fetched version are present.
- **Bottom-left button** is now state-driven for the not-installed case
  (`launcher/src/ui/bottom_bar.rs`): label switches between `Install
  <Channel> Game`, `Update` (Stage-4 placeholder), and `Installing…`
  while a download is in flight. The press is disabled while the game
  is running OR an install is in flight.

### Changed

- **`launcher/src/updater/mod.rs`** now declares `pub mod branches`
  alongside the existing self-update modules. Module docs updated to
  describe the new sibling.
- **`reqwest` features** gain `stream` (chunked download); `tokio` gains
  `fs` for async file I/O in the installer.

### Dev gating

- `Message::UpdatePressed` refuses to act on `Channel::Dev` when
  `state.dev_flag` is false (defence-in-depth — the channel selector
  already hides Dev under that condition).
- `Message::InstallConfirmed` re-checks the dev flag before kicking off
  the download.

### Deferred to later stages

- **Stage 4** — per-boot fan-out of `latest_release(channel)` into a
  cached `state.available_versions` map, the full bottom-left button
  state machine (Update to vX.Y.Z / Up to date — vX.Y.Z), and the top
  banner derivation from real version diffs.
- **Stage 5** — Play button wires through to spawning the game
  executable with the username temp-file handoff from Stage 1.
- **Stage 6** — bottom progress bar consumes the existing
  `InstallProgress::Downloading { fraction, bytes_now, bytes_total }`
  events (already emitted, currently only traced).

---

## [0.4.0] — 2026-05-23

Lights up the real identity + dev-flag pipeline. Until now `visible_channels`
was hardcoded in `mock.rs`, identity was a `mock_identity()` stub with no
file I/O, and the Settings → Game Channel Management section ignored
`visible_channels` entirely by iterating `Channel::all()`. v0.4.0 replaces
the mocks with a per-launch handshake against each channel server: identity
is loaded from disk (or freshly registered), the dev server's response
populates `dev_flag` for **this launch only**, and `state.visible_channels`
is recomputed accordingly. The launcher's Settings tab and left-rail
picker both now consume that list, so the Dev row is hidden everywhere
unless an operator has flipped the user's dev_flag in the new admin Users
tab on the dev server.

Requires the matching server v0.6.0 (idempotent `/register` shape +
`/admin/users`). Earlier servers will 4xx the boot register calls — the
launcher tolerates this by leaving the per-channel row marked unreachable
and keeping Dev hidden.

### Added

- **First-launch welcome screen** (`launcher/src/ui/welcome.rs`,
  `launcher/src/app.rs`). On a launch with no username on file
  (`state.identity.username.trim().is_empty()`), `view()` short-circuits
  to a full-window centered welcome card with a text input + Confirm
  button before the main 5-zone layout renders. Blank submissions are
  rejected (Confirm is disabled, and the `on_submit` Enter-key path
  defence-checks again). `boot()` holds back the 3 per-channel
  `/register` tasks until the welcome flow completes, so the server's
  first record of this user carries their chosen name rather than a
  placeholder. The identity file is persisted **before** the first
  /register call leaves the process, so a crash between Confirm and the
  server response still leaves a usable file on disk and the next boot
  skips straight to registration. Returning users (with a username on
  file from a prior launch) never see this screen.

- **`launcher/src/paths.rs`** (new). All launcher-managed user data lives
  under `<install_dir>/data/` next to the binary:
  `data/identity.json` for the credential file and `data/saves/<channel>/`
  reserved for the existing per-channel "Game Save" buttons. This is
  explicitly the "for the time being" choice — colocation is easier to
  inspect / back up / reset during pre-stable development. Known
  limitation: `.deb` installs land the binary in `/usr/bin/` which isn't
  user-writable; portable installs (tarball, NSIS) work fine. Hardened
  XDG / `%APPDATA%` placement is deferred.

- **Identity file I/O** (`launcher/src/identity.rs`). New `load()`
  reads `data/identity.json` (returns `Ok(None)` on first run); new
  `save()` writes atomically via tmp-file + rename and chmods `0600`
  on Unix. Parse / I/O failures fall through to fresh registration —
  a corrupted identity file is self-healing on next launch.

- **`launcher/src/server_api.rs`** (new). Thin reqwest-backed client for
  `POST /register` and `POST /me/username` against
  `https://{channel.host()}/...`. 10s timeout, rustls-tls only.

- **Per-launch `/register` calls** in `app::boot`
  (`launcher/src/app.rs`). On every launch — not just first run — the
  launcher fires three parallel `register` tasks alongside the existing
  GitHub update check, passing any prior creds it has cached so the
  server reuses the same `player_id`. The dev server's `dev_flag` field
  drives `state.dev_flag` and the recomputed `state.visible_channels =
  [Stable, Ea(, Dev if dev_flag)]`. Server is the source of truth; the
  flag is never persisted on the user's machine. The dev server being
  unreachable explicitly forces `dev_flag = false` (foundation §3
  visibility-matrix row "No / — / (unknowable) / No").

- **Username change fan-out** in `Message::ConfirmUsernameChange`. After
  the local rename + identity-file rewrite, the launcher fires one
  `update_username` task per channel where it has credentials. Failures
  are logged but don't block the UI — the next boot's `/register` will
  resync the server in any case.

### Changed

- **Settings → Game Channel Management actually consumes
  `visible_channels`** (`launcher/src/ui/center/settings.rs`).
  `channels_section()` and `important_files_section()` now iterate
  `&state.visible_channels` instead of `Channel::all()`. This is the bug
  the v0.4.0 work was named after — the dev row no longer appears in
  Settings for unflagged users.

- **`mock.rs` no longer fakes identity or visibility**. `mock_identity()`
  and `VISIBLE_CHANNELS` are gone. The only mock that remains is
  `BRANCH_UPDATES_AVAILABLE`, which a future slice will replace once the
  game-files update stream lands.

- **Default `visible_channels` is `[Stable, Ea]`**. Before any server
  response arrives — and on every cold launch — the Dev row stays hidden.

- **`Channel` gains a `dir_name()` const** returning the lowercase form
  used both in the serde rename and the new `data/saves/<channel>/`
  directory structure.

- **Version** 0.3.3 → 0.4.0. Minor bump — `data/identity.json` is a new
  file, the launcher now makes outbound HTTPS calls on every boot, and
  the launcher requires server v0.6.0. No migration of pre-v0.4.0
  installations is needed: there was no on-disk identity to migrate.

---

## [0.3.3] — 2026-05-23

Adds a third tab to the Settings center pane — **Launcher Options** — that
inline-renders the existing launcher update controls (current version,
availability cell, *Check for Updates* / *Start Update* buttons, status line).
Previously the launcher update view was only reachable via the top-bar
"Update available" banner, which is itself only clickable once GitHub
Releases reports a newer launcher. A user who wanted to verify their
installed version or manually re-run the update check had no entry point
when no update was being advertised. The new tab gives that flow a
permanent, always-accessible home in Settings.

### Added

- **Settings → Launcher Options tab** (`launcher/src/ui/center/settings.rs`,
  `launcher/src/app.rs`). New `SettingsTab::LauncherOptions` variant joins
  the existing `ChannelManagement` and `Graphics` tabs; selecting it renders
  the launcher update controls inline beneath the Settings tab bar. No new
  messages — the existing `CheckForUpdatesPressed` and
  `StartLauncherUpdatePressed` flows are reused as-is, so the in-flight /
  game-running / no-update-available safety gates still apply identically.

### Changed

- **`launcher_update::view` split into `view` + `content`**
  (`launcher/src/ui/center/launcher_update.rs`). The inner version /
  availability / buttons / status block moved into a new public
  `content(state)` helper. `view(state)` keeps wrapping it with the
  standalone "Launcher Update" header and `menu_pane` container so the
  top-bar banner entry point (`CenterView::LauncherUpdate`) is visually
  unchanged. The Settings tab calls `content(state)` directly to avoid a
  duplicate header / Close button inside the Settings pane.

- **Version** 0.3.2 → 0.3.3. Patch bump — UX-only addition. No changes to
  the update logic, GitHub Releases query, install layout, or any public
  API. The new entry point routes through the same code paths a top-bar
  banner click already used.

---

## [0.3.2] — 2026-05-22

Cleans up stale self-update artifacts that the `self_update` / `self-replace`
rename-trick can leave behind in the install directory when the Windows
deletion helper is killed (AV, crash, race with our own `process::exit(0)`
right after the swap). Before this release, files like
`.briskablast-launcher.<32-random>.__relocated__.exe` and
`.briskablast-launcher.<32-random>.__selfdelete__.exe` would accumulate
in the install dir over successive self-updates with no mop-up path.

### Fixed

- **Stale self-update orphans removed on startup**
  (`launcher/src/updater/cleanup.rs`). New
  `updater::cleanup_stale_update_artifacts()` runs once from `main`
  between `init_tracing()` and `iced::application(...)`. It scans the
  directory containing the running exe and removes files matching
  `.<current_exe_stem>.*` whose name ends in one of the three
  `self-replace` 1.5 suffixes: `.__relocated__.exe`, `.__selfdelete__.exe`,
  `.__temp__.exe`. Symlinks are skipped (`symlink_metadata` + `is_file()`),
  per-file failures are logged at `warn!` and swallowed so a locked file
  never blocks startup. Pattern is anchored on the current binary's stem
  rather than a hardcoded name, so a future renamed launcher binary still
  scopes correctly. Includes a tempdir-based unit test covering the
  three positive matches against three negative controls (the real exe,
  a non-artifact dotfile, an artifact for a different binary stem).

- **Version** 0.3.1 → 0.3.2. Patch bump — bugfix only, no API or install
  layout changes from v0.3.1. Lets a `launcher-v0.3.2-dev.N` Release
  parse as semver-greater than an installed v0.3.1.

### Notes

- Linux's `self-replace` overwrites the running exe via `tempfile` rename,
  so leftovers there are vanishingly rare (only on a final-rename failure,
  leaving a `.<stem>.__temp__*` file). Cleanup is harmless on Linux either
  way — the suffix list either matches a real failed-update orphan or
  no-ops.
- Cleanup runs unconditionally on every launch, not just after an update.
  A directory scan of a typical install dir is cheap, and the user-visible
  bug class (helper killed mid-cleanup) means the orphan can outlive any
  "did we just update" flag we'd otherwise gate on.

### Deferred (unchanged from 0.3.1)

See the `[0.3.0]` Deferred section.

---

## [0.3.1] — 2026-05-22

Hides the empty console window that Windows allocated behind the
launcher GUI on release builds. Same v0.3.0 install + self-update
mechanics; first real use of the `self_update` rename-trick swap will
upgrade an installed v0.3.0-dev.5 launcher to this release.

### Changed

- **Windows release builds run under the `windows` subsystem**
  (`launcher/src/main.rs`). Adds
  `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
  at the crate root so `cargo build --release` produces a GUI-subsystem
  PE on Windows — no more `conhost.exe` window behind the Iced window.
  Debug builds keep the console so `cargo run -p launcher` still shows
  tracing output during development. Non-Windows targets ignore the
  attribute entirely (no-op on Linux).

- **Version** 0.3.0 → 0.3.1. Needed so a `launcher-v0.3.1-dev.1` Release
  parses as semver-greater than the installed v0.3.0 binary (a
  `launcher-v0.3.0-dev.6` tag would parse as `0.3.0-dev.6`, which is
  semver-LESS than `0.3.0` because pre-release suffixes are ordered
  below the base version — so the running launcher would never offer
  the update).

### Notes

- Side effect on Windows release builds: anything `tracing` writes to
  stderr is dropped (no terminal attached). For deployed-launcher debug
  visibility, a file appender on `tracing-subscriber` is the future
  move; not in scope here.
- This is the first release that lets us exercise the full end-to-end
  self-update flow: install v0.3.0-dev.5's `setup.exe`, run it, click
  "Check for Updates" → banner appears → "Start Update" → rename-trick
  binary swap → relaunched binary's console window is gone.

### Deferred (unchanged from 0.3.0)

See the `[0.3.0]` Deferred section.

---

## [0.3.0] — 2026-05-22

First end-to-end install + self-update slice. The launcher now ships as a
real installable artifact on both Windows (NSIS-driven setup.exe) and Linux
(AppImage + `.deb`), and the in-app Launcher-Update menu's previously-stub
"Start Update" button performs a live binary swap against a GitHub Release
via the `self_update` crate's rename-trick. The launcher silently queries
GitHub Releases for a newer `launcher-v*` tag on boot and exposes a
"Check for Updates" button to re-run the query on demand. Game-launch and
game-server session wiring are intentionally untouched on this slice; the
focus is install / uninstall / self-update mechanics so we can iterate on
launcher releases without rebuilding installers by hand.

### Added

- **`launcher/src/updater/mod.rs` + `updater/github.rs`** — first real
  population of the previously-empty `updater/` module. Public surface is
  `check_for_update()` (async wrapper over `self_update::backends::github::ReleaseList`)
  and `run_self_update(version)` (async wrapper over
  `self_update::backends::github::Update`). Both internally
  `tokio::task::spawn_blocking` because `self_update` is sync. Discovery
  filters releases on the `launcher-v` tag prefix and parses the suffix
  with the `semver` crate — never string comparison
  (`launcher-update-and-version-validation.md` §Implementation Specifics).
  The three empty sub-subdirs (`branches/`, `downloader/`, `patcher/`)
  stay as `.gitkeep` placeholders — game-files-update territory, not in
  scope on this slice.

- **Cargo deps** (`launcher/Cargo.toml`):
  - `self_update = { version = "0.41", default-features = false, features = ["rustls", "compression-zip-deflate", "archive-zip"] }`
    — `rustls` keeps us off the OpenSSL build path; `archive-zip` is
    needed for the Windows self-update asset (the Linux side reuses the
    crate's tar/gz default).
  - `semver = "1"` — same crate the server already uses for version
    comparisons (`launcher-update-and-version-validation.md` §Server Side).
  - `tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"] }`
    — explicit dep for `spawn_blocking`. Iced already pulls tokio
    transitively via its `tokio` feature; calling out the dep makes the
    blocking-task usage discoverable from the manifest.

- **`[package.metadata.deb]`** (`launcher/Cargo.toml`) — `cargo-deb` config
  emitting `/usr/bin/briskablast-launcher` + `.desktop` + 512×512 hicolor
  icon. `depends = "$auto"` so runtime deps come from the binary's ELF
  dynamic-link table at build time. `section = "games"`. The
  AGPL-3.0-or-later top-level `LICENSE` is referenced verbatim with
  `["../LICENSE", "0"]`.

- **`AppState` boot task + new messages** (`launcher/src/app.rs`):
  - `boot()` returns `(AppState, Task<Message>)` and fires the first-launch
    `Task::perform(updater::check_for_update(), Message::LauncherUpdateCheckDone)`.
    Replaces the v0.2.0 `iced::application(AppState::default, …)` form.
  - New messages: `LauncherUpdateCheckDone(Result<UpdateCheckOutcome, String>)`,
    `CheckForUpdatesPressed`, `SelfUpdateDone(Result<(), String>)`.
  - New state fields: `launcher_release_notes`, `update_check_in_flight`,
    `self_update_in_flight`, `last_self_update_error`. These power the
    button-disable + status-text affordances in the Launcher-Update view.
  - The v0.2.0 stub `Message::StartLauncherUpdatePressed` (no-op at
    `app.rs:117`) is now a real handler: refuses with an inline error when
    `state.game_running` (the critical safety check from
    `launcher-update-and-version-validation.md:103-105`), kicks off
    `Task::perform(updater::run_self_update(version), Message::SelfUpdateDone)`
    otherwise. On `Ok(())` the handler `std::process::exit(0)`s so the
    swapped binary's clean relaunch is what the user sees next.

- **Launcher-Update center view rewrite** (`launcher/src/ui/center/launcher_update.rs`):
  - Adds a "Check for Updates" button next to "Start Update".
  - Three availability cells (update available / checking… / up to date)
    swap based on `state.update_check_in_flight` +
    `state.launcher_update_available`.
  - Buttons disable via the same `.on_press()`-omission pattern already
    used in `bottom_bar.rs` — no explicit `disabled()` API needed in iced
    0.14. Disable rules:
    - "Check for Updates" — disabled while a check OR a swap is in flight.
    - "Start Update" — disabled unless an update is known available, the
      game is not running, and nothing is already in flight.
  - Status line under the buttons surfaces (in priority):
    `last_self_update_error` → game-running warning →
    `launcher_release_notes` preview (first 220 chars of the GitHub
    Release body) → empty spacer.

- **Placeholder icon assets** (`launcher/assets/`):
  - `icon.svg` — vector source of truth (dark-navy rounded square, white
    accent ring, radial orange "blast" gradient, outlined-white "BB"
    letters in navy).
  - `icon.png` — 512×512 PNG used by AppImage + `.deb` hicolor.
  - `icon.ico` — 32 / 48 / 256 multi-size PNG-in-ICO for NSIS.
  - **`tools/build/gen-icon.py`** — pure-stdlib Python regenerator
    (no Pillow, no ImageMagick) that emits the PNG + ICO from
    procedural geometry matching the SVG. Drop-in: edit the SVG +
    geometry constants in the script, rerun `python3 tools/build/gen-icon.py`,
    commit. Chosen over a Rust-crate-driven renderer to avoid pulling
    `tiny-skia` / `resvg` into the launcher build; chosen over
    ImageMagick at CI time to keep workflow YAML small.

- **`launcher/assets/briskablast-launcher.desktop`** — XDG desktop entry
  consumed by both `linuxdeploy` (AppImage) and `cargo-deb`. `Type=Application`,
  `Categories=Game;`, `StartupWMClass=BriskaBlast Launcher`.

- **NSIS installer hardened** (`tools/installer/launcher.nsi`):
  - `Icon` + `UninstallIcon` uncommented, pointing at the new
    `launcher/assets/icon.ico`.
  - Start Menu shortcut now created under `$SMPROGRAMS\BriskaBlast\`.
  - Add/Remove Programs registry gains `DisplayVersion`, `DisplayIcon`,
    `Publisher`, `URLInfoAbout`, `InstallLocation`, `NoModify`, `NoRepair`.
    `VERSION` is `/D`-defined by CI; defaults to `0.0.0` for hand runs.
  - **Uninstaller does not touch `%APPDATA%\BriskaBlast\`.** Identity file
    (`player_id` + `secret_token` per channel) survives uninstall —
    losing it means losing the account permanently
    (`launcher-update-and-version-validation.md` §Uninstall Considerations).
    A future branch adds the opt-in "Keep player data?" prompt once
    identity-file I/O lands.

- **`release-launcher.yml` reshaped** to produce `self_update`-compatible
  assets and publish on tag push:
  - Trigger gains `push: tags: ['launcher-v*']`; `workflow_dispatch` kept
    for hand builds (publish step gates on `startsWith(github.ref, 'refs/tags/launcher-v')`).
  - Linux artifact set:
    `briskablast-launcher-<ver>-x86_64-unknown-linux-gnu.tar.gz` (self_update
    target asset), `briskablast-launcher-<ver>-linux.AppImage` (portable),
    `target/debian/briskablast-launcher_<ver>_amd64.deb` (apt-style).
  - Windows artifact set:
    `briskablast-launcher-<ver>-x86_64-pc-windows-msvc.zip` (self_update
    target asset), `BriskaBlast-Launcher-Setup-<ver>.exe` (NSIS installer).
  - `makensis` invoked as `makensis /DVERSION=$VERSION …`.
  - Linux job installs `cargo-deb` and runs `cargo deb -p launcher --no-build`.

### Changed

- **Binary name** `launcher` → `briskablast-launcher`. `[[bin]] name`
  bumped in `launcher/Cargo.toml`. Eliminates the v0.2.0 mismatch where
  `release-launcher.yml:34` and `tools/installer/launcher.nsi:12` already
  expected `briskablast-launcher` but Cargo produced `launcher`.

- **Version** `0.2.0` → `0.3.0`. Required so a `launcher-v0.3.0-dev.1`
  GitHub Release published from this branch reports as newer than a
  still-running v0.2.0 binary during self-update smoke tests.

- **`launcher_update_available` / `launcher_available_version`** in
  `AppState` no longer initialize from `mock::*` constants — they're
  populated at runtime by the boot-time GitHub Releases query.

### Removed

- **`mock::LAUNCHER_UPDATE_AVAILABLE`** and **`mock::LAUNCHER_AVAILABLE_VERSION`** —
  replaced by the real updater query. Comment block in `mock.rs` documents
  the swap so a future contributor can grep for the old names.

### Tag schema

Launcher releases use the `launcher-v<semver>` prefix to stay clear of the
server's existing `v*.*.*-dev.N` schema (which `release-server.yml`
validates against `server/Cargo.toml`). Today this is one shared release
stream regardless of channel — `dev` only. If per-channel launcher
pre-releases ever diverge, the prefix lets `self_update` keep using a
single filter while we layer in `launcher-v…-ea.N` / `launcher-v…-stable`
without breaking discovery.

### Verification

- `cargo check -p launcher` — clean.
- `cargo clippy -p launcher --no-deps` — clean (two transient warnings
  fixed: `field_reassign_with_default` in `boot()`, `unnecessary_map_or` in
  the updater's release ranking).
- `cargo build -p launcher --release` — produces
  `target/release/briskablast-launcher`, the name both the NSIS template
  and `release-launcher.yml` expect.
- `./target/release/briskablast-launcher` — process stays alive 5+
  seconds on WSL2 + WSLg under SIGTERM-timeout; no panic.
- Icon generator: `python3 tools/build/gen-icon.py` round-trips and `file
  launcher/assets/icon.{png,ico}` reports valid 512×512 PNG and
  multi-size MS Windows icon resource (32/48/256).

### Notes

- self_update's default target detection uses `env!("TARGET")` baked at
  compile time, which matches the `x86_64-unknown-linux-gnu` /
  `x86_64-pc-windows-msvc` substrings in the new CI artifact names — no
  override needed in `updater/github.rs`.
- `self_update`'s rename-trick cleanup of the orphaned old binary
  happens on the NEXT launch after the swap, not the current one. We
  `std::process::exit(0)` immediately after a successful swap to let that
  next-launch cleanup run cleanly.
- The "Updates available: launcher" top-bar banner reads from
  `state.launcher_update_available`; no top-bar code change was required.
- `launcher/src/main.rs` still uses iced 0.14's synchronous `main()` —
  Iced owns the runtime, we just hand it `app::boot` (the new tuple form),
  `app::update`, `app::view`.

### Deferred (not in this release)

- Identity file I/O — installers ignore `%APPDATA%\BriskaBlast\` /
  `~/.config/briskablast/` on uninstall as the v1 policy; the "Keep
  player data for future reinstall?" prompt waits on the identity-file
  read/write slice (`launcher-foundation.md` §8 Open Items).
- Server-side HTTP 426 "update required" prompt — server still serves
  it; launcher does not consume it yet (`launcher-foundation.md` §5I).
- Game-launch + game-process supervision.
- Per-channel game-files update flow (`updater/branches/`, `downloader/`,
  `patcher/` still empty).
- Markdown rendering of GitHub Release `body` — current view shows a raw
  220-char preview.
- macOS support — not on platform list per `CLAUDE.md` §Platforms.
- Snapshot or integration tests for the updater module — `launcher/tests/`
  remains empty, consistent with the v0.1.0 / v0.2.0 baseline.

---

## [0.2.0] — 2026-05-21

Three routable center-pane views land: a Settings page (Channel
Management + Graphics tabs), a Change-Username form, and a Launcher
Update prompt. The standard 5-zone layout is unchanged; only the
center pane swaps content based on a new `CenterView` enum on
`AppState`. The gear button and the right-rail "Change Name" button —
both inert in v0.1.0 — now open their respective views, and clicking
the "Update available: launcher" banner opens the new update menu.
Confirm Change actually mutates `AppState.identity.username` in
memory; the Start Update button in the launcher-update menu is a stub
— the real `self_update` flow ships in a follow-up branch.
Identity-file persistence also still ships in a later slice.

### Added

- **`CenterView` + `SettingsTab` enums** (`launcher/src/app.rs`) — view
  routing state. `AppState.center_view` defaults to `CenterView::Default`
  so existing behavior is preserved when nothing is open.

- **Eight new `Message` variants** for menu lifecycle:
  `CloseCenterMenu`, `SettingsTabSelected`, `UsernameDraftChanged`,
  `ConfirmUsernameChange`, `UninstallChannel`, `VerifyChannel`,
  `GameSavePressed`, `StartLauncherUpdatePressed`. `update()` mutates
  state for the lifecycle messages and intentionally no-ops the four
  stub action messages (continues the v0.1.0 "buttons exist but only
  log clicks" convention). `LauncherUpdatePressed` (already defined in
  v0.1.0) is wired to open the new launcher-update view.

- **`launcher/src/ui/center/` directory module** — `mod.rs` routes on
  `state.center_view` to one of three sibling files:
  - `default.rs` — original "Briska Blast / No menu selected" placeholder
    (lifted verbatim from the old `center.rs`, no behavior change).
  - `settings.rs` — header + tab bar + active-tab body. Channel
    Management tab renders a Channels grid (`Uninstall` + `Verify File
    Integrity` per channel) and a Game Important Files grid (`Game Save`
    per channel + reserved third cell). Graphics tab renders a
    `Coming soon.` placeholder.
  - `change_username.rs` — current-name display cell, `text_input` for
    the new name, Confirm Change button (disabled by omitting
    `.on_press()` when the trimmed draft is empty — same disable pattern
    as `bottom_bar.rs` Update/Play when the game is running), and a
    Close button.
  - `launcher_update.rs` — current vs available version display
    (current from `env!("CARGO_PKG_VERSION")`, available from
    `AppState.launcher_available_version`) and a Start Update button
    that's wired to `StartLauncherUpdatePressed` (stub — no real
    download yet). Close returns to default.

- **`theme::tab_active` / `theme::tab_inactive`** — `button::Style`
  helpers in the same `Color::from_rgba(1.0, 1.0, 1.0, _)` palette
  already used by `bordered` and `menu_pane`. Active tab is filled at
  18% white with a thicker border; inactive is outline-only at 30%.

- **First `iced::widget::text_input` use** in the launcher crate
  (Change-Username form). No Cargo feature change needed — iced 0.14
  ships `text_input` by default.

- **`mock::LAUNCHER_AVAILABLE_VERSION`** (`launcher/src/mock.rs`) — string
  constant paired with the existing `LAUNCHER_UPDATE_AVAILABLE: bool`.
  Surfaces in `AppState.launcher_available_version` for the new view.

- **Version bump** `0.1.0 → 0.2.0` (`launcher/Cargo.toml`). The top-left
  version cell in `top_bar.rs` reflects this via
  `env!("CARGO_PKG_VERSION")`.

### Verification

- `cargo check -p launcher` — clean, no warnings. Three new `Message`
  variants that take `Channel` payloads are annotated
  `#[allow(dead_code)]` until real handlers consume them (matches the
  v0.1.0 precedent on `Channel::host`/`all`).
- `cargo build -p launcher` and `cargo run -p launcher` — manual
  exercise of all flows: gear → Settings opens; tab switch swaps body;
  Close returns to default; Change Name opens form pre-filled with
  current username; typing enables Confirm; Confirm mutates the right-
  rail username and closes the form; clearing the input re-disables
  Confirm.

### Notes

- The action buttons in Settings (Uninstall / Verify / Game Save) are
  stubs — they log via the existing `tracing::debug!(?message, ...)` in
  `update()` and otherwise do nothing. Same status as Play / Update /
  LauncherUpdate in v0.1.0.
- Confirm Change mutates only the in-memory `Identity`. The change does
  not survive a launcher restart until the identity-file I/O slice
  lands (`identity.rs:2` still says "file I/O lands in a later slice").
- Mockups for both menus are at `Example Imgs/LuncherSettings.png` and
  `Example Imgs/UserNameChange.png`.

### Deferred (not in this release)

- Identity-file persistence — confirmed username changes vanish on
  launcher restart for now.
- Real Uninstall / Verify File Integrity / Game Save logic — UI stubs
  only this release.
- Real `self_update`-crate-driven launcher self-update flow. The new
  launcher-update menu's Start Update button is a stub that logs only;
  the real rename-trick replacement, GitHub Releases integration,
  progress reporting, and `game_running` safety gate
  (`docs/launcher/launcher-update-and-version-validation.md` §§Update
  flow + §Critical safety) ship in a dedicated follow-up branch.
- Graphics tab content beyond "Coming soon."
- Username validation / uniqueness rules
  (`docs/launcher/launcher-foundation.md:269`).
- Snapshot or integration tests for the new views — `launcher/tests/`
  remains empty; consistent with the v0.1.0 baseline.

---

## [0.1.0] — 2026-05-21

First versioned launcher build. UI scaffold + data model only — no file
I/O, no network calls, no settings panel, no update logic. Establishes
the package shape, the 5-zone window layout, the identity-file schema
as compile-checked Rust types, and the channel taxonomy. Buttons exist
but only log clicks at debug level.

### Added

- **Foundation design doc** at `docs/launcher/launcher-foundation.md` —
  the spec this code implements. Covers the 5-zone layout, the local
  identity file shape, channel visibility gating (dev hidden behind a
  server-side per-user flag), nine UI state variants, the two-stream
  update model (launcher self-update vs game-files-per-channel), the
  planned A/B install slots successor, server-status panel meanings,
  and explicitly deferred items.

- **Workspace integration** — `launcher` added to the root `Cargo.toml`
  `[workspace] members`. Edition 2021, resolver `"2"` matching `server/`
  and `shared/`.

- **Iced 0.14 application** — synchronous `main()` (Iced owns the async
  runtime via its `tokio` feature). `iced::application(boot, update,
  view).title(...).theme(Theme::Dark).run()` pattern.

- **5-zone layout** (`launcher/src/ui/`):
  - **Top bar** — launcher version (`env!("CARGO_PKG_VERSION")`),
    branch-updates banner, launcher-update banner, gear icon button.
  - **Left rail** — channel picker + server-status dots panel. Dev row
    filtered out when not in `state.visible_channels`.
  - **Center pane** — title + "no menu selected" placeholder. Styled
    with a subtly-darker background and thicker border to mark it as
    the menu-display surface.
  - **Right rail** — username display + Change Name button + per-channel
    Player IDs list (dev row hidden for unflagged users).
  - **Bottom bar** — Update button, progress placeholder (mock 35%
    complete), Play button.

- **Boxed sub-element styling** — each logical sub-element within a
  zone is wrapped in a bordered container so the layout matches the
  hand-drawn foundation mockup at `Example Imgs/Luncher Design.png`.
  Two `container::Style` helpers in `ui/theme.rs`: `bordered` (thin
  1.5px white @ 40% alpha border, no fill) and `menu_pane` (2px @ 55%
  border + 6% white-alpha fill).

- **Data model** (top-level `launcher/src/`):
  - `channel.rs` — `Channel` enum (`Stable` / `Ea` / `Dev`) with serde
    `rename_all = "lowercase"`. `Channel::host()` returns the baked-in
    hostnames matching `client/BriskaBlast.csproj`'s GenerateBuildConfig
    target. `Channel: Ord` by discriminant gives canonical `stable → ea
    → dev` iteration order.
  - `identity.rs` — Serde `Identity { username, channels:
    BTreeMap<Channel, ChannelCreds> }` matching foundation doc §2.
    `BTreeMap` (not `HashMap`) preserves key order on serde roundtrip.
  - `mock.rs` — sole source of v1 fake reality: one shared username
    ("BlastQueen99"), per-channel mock player IDs, visible-channels
    list (`[Stable, Ea]` — unflagged-user mock), update-available
    state, progress percentage.

- **Application glue** (`launcher/src/app.rs`) — `AppState` view-model,
  closed `Message` enum (PlayPressed, UpdatePressed, OpenSettings,
  ChannelPicked, ChangeNamePressed, LauncherUpdatePressed), `update`/
  `view`/`theme`/`title`. `AppState::default()` constructs from
  `mock::*` constants so the v1.x I/O slice has a single replacement
  point.

- **Tracing init** mirroring `server/src/main.rs:26-32` — registry →
  `EnvFilter` (default `launcher=info`) → `fmt::layer`. Every Message
  in `update()` is logged at debug.

- **Channel taxonomy alignment** — `experimental` renamed to `ea`
  across `docs/dev/devtools.md`, `docs/server/server-autoupdate.md`,
  and `readme.md`. Resolves the cross-doc naming inconsistency where
  the launcher doc used `experimental` while the server / client /
  CI already used `ea`.

- **CLAUDE.md** — index row pointing at `launcher-foundation.md` under
  "Where to Find Information."

### Verification

- `cargo check -p launcher` — clean, no warnings (dead-code on
  `Channel::host`/`all` annotated with `#[allow(dead_code)]` until the
  network slice consumes them).
- `cargo build -p launcher` — full debug build in ~47s cold
  (`wgpu` + `winit` + glyph stacks).
- `cargo run -p launcher` on WSL2 + WSLg — window opens cleanly; runs
  uninterrupted for 6-8s under timeout-driven SIGTERM (exit 143, not a
  panic).

### Notes

- v1 buttons are intentionally inert; only `ChannelPicked` mutates
  state. Each click logs at debug via `tracing::debug!(?message, ...)`.
- Iced 0.14 expects `Pixels: From<u32>` or `From<f32>` — `u16` does
  not satisfy. Theme constants are `u32`.
- WSL2 dev environments may need `WINIT_UNIX_BACKEND=x11` or the
  `tiny-skia` feature on `iced` if `wgpu` + Vulkan can't initialize.
  Windows / native Linux not affected.
- Module subdirs `auth/`, `settings/`, `news/`, `networking/`,
  `updater/`, `config/`, `devtools/` keep their `.gitkeep` placeholders
  — no fake stubs. They light up as future slices populate them.
- `channel.rs`, `identity.rs`, `mock.rs` live at top-level `src/` for
  v1; they migrate into `auth/` / `config/` when those modules light
  up.

### Deferred (not in this release)

The following are scoped into future v1.x slices; not in this PR:

- Identity file I/O (read/write `identity.json` at a platform-
  appropriate path).
- Platform-paths crate selection (`dirs` vs `directories`).
- First-launcher-launch parallel reach-out to all three channel
  servers (depends on server endpoint design).
- Dev-flag retrieval from the dev server.
- Settings panel content.
- Update flow logic (game-files update + launcher self-update via the
  `self_update` crate).
- Channel switch confirmation modal.
- HTTP 426 "update required" prompt.
- Button coloring (Update / Play green, progress bar blue/purple) to
  finish matching the mockup.
- A/B install slots — the planned successor to the v1 "refuse to
  launch during update" rule (foundation doc §7).
- WS-ticket auth and TURN credentials delivery
  (`docs/planning/roadmap.md`).
