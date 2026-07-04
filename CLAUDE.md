# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Briska Blast is a cross-platform multiplayer online game targeting Windows, Linux, and macOS.
Packages: `client`, `launcher`, `server`, `shared`, `tools`.

## Technology Stack

| Component | Technology |
|---|---|
| Game Client | Godot 4 + C# |
| Launcher | Rust + Iced |
| Server | Rust + Axum |
| Async Runtime | Tokio |
| Session Storage | Redis |
| Containerization | Docker + Docker Compose |
| CI/CD | GitHub Actions |
| Process Management | Systemd / Docker restart policies |
| Admin Interface | Built-in admin panel at `/admin` |

**Platforms**: Windows + Ubuntu/Linux + macOS (Apple Silicon + Intel, universal builds via CI)

## Build Status

| Component | Status |
|---|---|
| Server foundation | ✓ Complete (current v0.21.0 — + observability: per-session tracing spans, signaling-relay trace, structured peer-failure logging, `LOG_FORMAT=json`) — see `ServerChangeLog.md` |
| Shared crate | ✓ Complete (v0.5.0 — protocol types, player/session types, shared `MAX_USERNAME_LEN` cap + `UpdateUsernameResponse` + `WinCondition` ("Set Score" target, range 10–50, default 11) + `SpawnSettings` (BallSpliter spawn interval 5–60s/default 15 + chain-split toggle)) |
| Game client | In progress (v0.20.0) — multiplayer lobby (bordered, responsive panels + server-relayed lobby chat) + WebRTC mesh behind `IPeerTransport` + a playable **Extended-mode** round (per-screen sim, ball handoff between screens, server-relayed scoring, **seat-relative portal layout** seated by join order from a server-frozen roster, matching the canonical seating diagram) + server-authoritative host promotion with a reconnect grace window + **usernames** in the lobby roster and scoreboard (server-resolved, `player_id` stays internal) + **single-instance / one-channel-at-a-time** via a socket-rendezvous autoload (`SingleInstance`, shared `game_instance.json`) + a **"Set Score" win condition** (host-configured in Advanced Settings → Match Rules, default 11) with a server-driven `GameOver` end-game leaderboard screen that freezes the sim + **per-channel .NET runtime-cache isolation** (`data_BriskaBlast{,EA,Dev}_<platform>`, set at export time) + build-time **`files.json`** integrity manifests + a **ball-splitter mechanic** (system-spawned **BallSpliter** splits the master ball into 3 double-value **BallBT** balls fanned 45° apart; host-tuned **Random Spawns** tab — splitter interval + chain-split; central `SpriteRegistry` asset lookup; multi-ball sim) + **corner barriers** (a solid L-shaped obstacle in all four corners of every screen — one sprite rotated 90° per corner about its bottom-left pixel; balls bounce off, turning shots away from the goal corners and stopping corner-cutting; collision + sprite placement share the `CornerBarrier` layout helper so they can't drift; new `AssetCategory.SystemControlled`) + a **structured logging system** (leveled/categorized `Log` autoload writing one per-run file per launch into a per-channel folder `log{dev,ea,stable}`, with WebRTC/handoff instrumentation for diagnosing peer-connection failures). See `GameChangeLog.md`, `docs/architecture/extended-mode.md`, `docs/architecture/observability-logging.md`, and `docs/planning/multiplayer-client-stages.md` |
| Launcher | In progress (v0.20.0) — **per-OS self-update** (Windows exe swap unchanged; macOS whole-`.app`-bundle swap keeping the ad-hoc seal valid, via a new `-macos-app.tar.gz` CI asset + local `codesign` verify/re-sign; Linux bare-binary tar.gz swap fixed by enabling `self_update`'s tar features + AppImage outer-file replacement; `.deb` installs get a "download the new .deb" message) + identity (+ game handoff with a `data_dir` field), multi-channel install/update (install locations inside the launcher's own folder are refused; on an **update** of an already-installed channel the install directory is **locked** to the existing location — the "Choose…" picker is disabled and re-enables only for a first-time install or after an uninstall), self-update, Windows firewall prompt, manual per-channel update check under the channel selector, GitHub rate-limit back-off safety net (+ `per_page=100` discovery), **20-char username cap** (hard-blocked in UI + server-enforced trust boundary, with a server-reverts-tampered-clients path), **username changes locked while a game is running**, and **socket-rendezvous single-instance** — ephemeral `127.0.0.1:0` bind + discovery file + handshake banner (`launcher_instance.json`); cross-restart game liveness now probes the game's `game_instance.json` socket (replacing the old `running_game.json` PID file / `sysinfo`), a **file-integrity / repair toolkit** (deep Verify File Integrity via `files.json`, Repair = fetch-by-tag reinstall, Windows-only **Reset Runtime Cache**), and a **dynamic scrollbar** on any center/menu page whose content overflows the visible pane (mouse-wheel when hovered + drag; headers and the Settings tab bar stay pinned) via a shared `ui/center/scroll_area` helper, and a **Logs button** (Settings → Game Channel Management, beside Game Save) that opens a channel's per-run game-log folder (`log{dev,ea,stable}`); see `LauncherChangeLog.md` |

**Build order:** Server → Game → Launcher (each depends on the previous).

## Where to Find Information

| Topic | File |
|---|---|
| Full package structure and module layout | [`docs/architecture/architecture.md`](docs/architecture/architecture.md) |
| Dev environment setup and Docker instructions | [`docs/dev/setup.md`](docs/dev/setup.md) |
| Manual testing with curl, admin panel tests | [`docs/dev/testing.md`](docs/dev/testing.md) |
| Server endpoint design and WebRTC signaling flow | [`docs/architecture/protocol.md`](docs/architecture/protocol.md) |
| Full networking, identity, and game design | [`docs/architecture/game-architecture-summary.md`](docs/architecture/game-architecture-summary.md) |
| Extended game mode — rules, ball handoff, scoring | [`docs/architecture/extended-mode.md`](docs/architecture/extended-mode.md) |
| File integrity, repair & per-channel runtime cache (Verify/Repair/Reset, `files.json`, the Godot assembly-rename gotcha) | [`docs/architecture/runtime-cache-and-integrity.md`](docs/architecture/runtime-cache-and-integrity.md) |
| Logging & observability (client per-run log files, WebRTC/handoff tracing, server per-session spans, `LOG_FORMAT`) | [`docs/architecture/observability-logging.md`](docs/architecture/observability-logging.md) |
| Asset/sprite registry (sprite lookup table, `AssetId` + category, adding a sprite) | [`docs/architecture/asset-registry.md`](docs/architecture/asset-registry.md) |
| Launcher self-update and version enforcement | [`docs/launcher/launcher-update-and-version-validation.md`](docs/launcher/launcher-update-and-version-validation.md) |
| Launcher UI layout, identity file, channel gating, state variants | [`docs/launcher/launcher-foundation.md`](docs/launcher/launcher-foundation.md) |
| Dev branch and release channel rules | [`docs/dev/devtools.md`](docs/dev/devtools.md) |
| Release tag namespaces (server / launcher / game) | [`docs/dev/release-tagging.md`](docs/dev/release-tagging.md) |
| Shared crate change history | [`SharedChangeLog.md`](SharedChangeLog.md) |
| Server change history | [`ServerChangeLog.md`](ServerChangeLog.md) |
| Game change history | [`GameChangeLog.md`](GameChangeLog.md) |
| Launcher change history | [`LauncherChangeLog.md`](LauncherChangeLog.md) |
| Deferred work and post-deployment follow-ups | [`docs/planning/roadmap.md`](docs/planning/roadmap.md) |
| Known bugs in current builds | [`docs/planning/known-bugs.md`](docs/planning/known-bugs.md) |
| Multiplayer client staged build order (lobby → WebRTC → gameplay) | [`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md) |

## Key Design Constraints

- `shared/` is a Rust library crate — platform-agnostic, no OS-specific built-ins. See [`docs/architecture/protocol.md`](docs/architecture/protocol.md).
- Dev tools ship on the `dev` branch only. See [`docs/dev/devtools.md`](docs/dev/devtools.md).
- Server runtime config (`min_launcher_version`, `min_game_version`, admin password) lives in Redis and is managed via the admin panel — not hardcoded.
- Bind address and ports (`GAME_PORT`, `ADMIN_PORT`) are deployment-time config via `.env` / `docker-compose.yml` — not managed at runtime.
- Default admin password is `@admin` — seeded on first boot, must be changed immediately.
- Player IDs are issued atomically: `/register` reuses the **lowest** freed number from the `player:freelist` pool (`ZPOPMIN`) if any, else increments `player:counter` (`INCR`). The counter is monotonic — never decremented — so issued-id totals still climb. Numbers are freed back into the pool when an admin deletes a user in the Users tab. Tokens use SHA-256. Admin password uses bcrypt.
- Version comparisons always use the `semver` crate — never string comparison.
- Client server hostname is **compile-time baked per channel** via `client/src/core/BuildConfig.cs`, generated by the `GenerateBuildConfig` MSBuild target in `client/BriskaBlast.csproj` from `RELEASE_CHANNEL` (`dev` / `ea` / `stable`). Do not introduce runtime hostname configuration — channel isolation is enforced at the build artifact level. Mirrors the server's `RELEASE_CHANNEL` pattern (`server/build.rs`).
