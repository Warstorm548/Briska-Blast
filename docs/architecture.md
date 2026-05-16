# Architecture

BriskaBlast is split into four top-level packages plus build tooling.

## Client (`client/`)
Godot 4 + C# project using Godot's scene/node tree. Game logic lives in C# scripts attached to nodes.
- `scenes/` — `.tscn` scene files (game, UI, menus, HUD)
- `scripts/` — C# scripts mirroring the scene hierarchy
  - `game/` — game loop, entities, state machines
  - `networking/` — WebRTC peer connections, server message handling
  - `input/` — keyboard/mouse/gamepad handling
  - `audio/` — sound playback and audio bus management
  - `ui/` — menu, HUD, and overlay logic
- `assets/` — sprites, fonts, shaders, audio, config
- `addons/` — Godot plugins and third-party addons

## Server (`server/src/`)
Rust + Axum server on Tokio. Handles player identity, session signaling, version enforcement, and the admin panel. Session state is backed by Redis.

Two independent `TcpListener` instances run inside the same process and share `AppState`:
- **Game listener** (`GAME_PORT`, default `25919`) — serves all player-facing endpoints
- **Admin listener** (`ADMIN_PORT`, default `25920`) — serves all `/admin/*` endpoints exclusively

**Built:**
- `main.rs` — entry point; builds two independent routers, binds two listeners, wires graceful shutdown via broadcast channel
- `config.rs` — environment variable loading with defaults (`GAME_PORT`, `ADMIN_PORT`, etc.)
- `error.rs` — unified `AppError` type implementing `IntoResponse`
- `state.rs` — shared `AppState` holding Redis pool and rate limiters (shared across both listeners)
- `api/` — HTTP route handlers (game listener only)
  - `register.rs` — `POST /register` — player identity issuance
  - `host.rs` — `POST /host` — session creation and code generation
  - `join.rs` — `POST /join` — session join and joiner endpoint exchange
  - `session.rs` — `GET /session/{code}` and `DELETE /session/{code}`
- `middleware/` — Tower middleware
  - `version.rs` — `X-Launcher-Version` and `X-Game-Version` enforcement (HTTP 426)
- `admin/` — password-protected web admin panel (admin listener only)
  - `auth.rs` — login, logout, bcrypt session handling
  - `dashboard.rs` — dashboard display and config update handlers
  - `templates.rs` — HTML page functions (no template engine dependency)

**Planned (future milestones):**
- `relay/` — real-time message relay between players once P2P is established
- `session/` — in-game session state management (scores, host promotion, reconnection)
- `matchmaking/` — lobby and player matching logic

## Shared (`shared/`)
Rust library crate shared between `server/` and `launcher/`. No OS-specific built-ins.
The Godot client uses equivalent C# types defined in `client/scripts/`.
- `src/protocol/messages.rs` — request/response types for all server endpoints
- `src/types/player.rs` — `PlayerId` type with sequential formatting
- `src/types/session.rs` — `SessionStatus` enum
- `src/utils/` — pure utility functions

## Launcher (`launcher/`)
Rust + Iced standalone binary that runs before the game. See [`devtools.md`](devtools.md) for the dev branch channel.
- `src/ui/` — launcher window, screens, layout components
- `src/auth/` — login, account creation, token/session storage
- `src/updater/` — core update engine
  - `branches/` — manifest fetching and branch switching
  - `downloader/` — file fetching and integrity verification
  - `patcher/` — applying diffs and binary swapping
- `src/news/` — patch notes feed, server status, announcements
- `src/settings/` — launcher preferences and game launch options
- `src/devtools/` — dev-branch-only overlay (hidden by default)
- `src/networking/` — shared HTTP client and CDN helpers
- `src/config/` — runtime config, env vars, launch flags
- `assets/` — launcher-specific backgrounds, icons, fonts
- `tests/` — launcher integration tests

## Infrastructure
- **Docker + Docker Compose** — server and Redis run in containers for portable redeployment
- **Redis** — session storage, player registry, and runtime config with TTL auto-expiry; `appendonly yes` ensures the player counter survives restarts
- **Portainer** — web GUI for container management (start/stop/logs/env vars)
- **Admin Panel** — password-protected web UI at `/admin` (admin port only) for managing runtime config (version gates, password) without container restarts
- **Systemd / Docker restart policies** — keeps containers alive across reboots
- **GitHub Actions** — CI/CD: auto-build and deploy on push to main

## Tools (`tools/`)
- `build/` — build scripts and bundler configuration
- `dev/` — local dev helpers (hot reload, dev server, etc.)
