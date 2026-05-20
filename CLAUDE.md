# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Briska Blast is a cross-platform multiplayer online game targeting Windows and Linux.
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

**Platforms**: Windows + Ubuntu/Linux

## Build Status

| Component | Status |
|---|---|
| Server foundation | ✓ Complete — see `ServerChangeLog.md` |
| Shared crate | ✓ Complete (protocol types, player/session types) |
| Game client | Not started |
| Launcher | Not started |

**Build order:** Server → Game → Launcher (each depends on the previous).

## Where to Find Information

| Topic | File |
|---|---|
| Full package structure and module layout | [`docs/architecture.md`](docs/architecture.md) |
| Dev environment setup and Docker instructions | [`docs/setup.md`](docs/setup.md) |
| Manual testing with curl, admin panel tests | [`docs/testing.md`](docs/testing.md) |
| Server endpoint design and hole-punch flow | [`docs/protocol.md`](docs/protocol.md) |
| Full networking, identity, and game design | [`docs/game-architecture-summary.md`](docs/game-architecture-summary.md) |
| Launcher self-update and version enforcement | [`docs/launcher-update-and-version-validation.md`](docs/launcher-update-and-version-validation.md) |
| Dev branch and release channel rules | [`docs/devtools.md`](docs/devtools.md) |
| Server change history | [`ServerChangeLog.md`](../ServerChangeLog.md) |
| Deferred work and post-deployment follow-ups | [`docs/roadmap.md`](docs/roadmap.md) |

## Key Design Constraints

- `shared/` is a Rust library crate — platform-agnostic, no OS-specific built-ins. See [`docs/protocol.md`](docs/protocol.md).
- Dev tools ship on the `dev` branch only. See [`docs/devtools.md`](docs/devtools.md).
- Server runtime config (`min_launcher_version`, `min_game_version`, admin password) lives in Redis and is managed via the admin panel — not hardcoded.
- Bind address and ports (`GAME_PORT`, `ADMIN_PORT`) are deployment-time config via `.env` / `docker-compose.yml` — not managed at runtime.
- Default admin password is `@admin` — seeded on first boot, must be changed immediately.
- Player IDs are sequential and atomic (Redis `INCR`). Tokens use SHA-256. Admin password uses bcrypt.
- Version comparisons always use the `semver` crate — never string comparison.
