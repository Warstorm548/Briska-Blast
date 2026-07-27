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
| Server foundation | ✓ Complete (current v0.32.0 — see `ServerChangeLog.md`) |
| Shared crate | ✓ Complete (v0.6.0 — see `SharedChangeLog.md`) |
| Game client | In progress (v0.30.0 — see `GameChangeLog.md`) |
| Launcher | In progress (v0.20.1 — see `LauncherChangeLog.md`) |

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
| Match lifecycle — MatchFlow state machine, lobby→game handoff, Preparing phase | [`docs/architecture/match-lifecycle.md`](docs/architecture/match-lifecycle.md) |
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
