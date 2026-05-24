# Launcher Changelog

All notable changes to the Briska Blast launcher are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
