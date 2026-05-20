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
Rust + Axum server on Tokio. Handles player identity, session signaling, version enforcement, the admin panel, and self-managed container updates. Session state is backed by Redis.

Two independent `TcpListener` instances run inside the same process and share `AppState`:
- **Game listener** (`GAME_PORT`, default `25919`) — serves all player-facing endpoints
- **Admin listener** (`ADMIN_PORT`, default `25920`) — serves all `/admin/*` endpoints exclusively

**Built:**
- `main.rs` — entry point; builds two independent routers, binds two listeners, spawns the update background task, wires graceful shutdown via broadcast channel
- `build.rs` — compile-time script that bakes `RELEASE_CHANNEL` into the binary via `env!("RELEASE_CHANNEL")`
- `config.rs` — environment variable loading with defaults (`GAME_PORT`, `ADMIN_PORT`, `WATCHTOWER_URL`, `WATCHTOWER_TOKEN`, etc.)
- `error.rs` — unified `AppError` type implementing `IntoResponse`; structured 4xx variants (`InvalidPlayerCount`, `SessionFull`, `SessionNotStartable`) carry context the client can render directly
- `state.rs` — shared `AppState` holding Redis pool, rate limiters, `update_tx` channel sender, `update_apply_lock` (single-flight mutex serialising every code path that triggers Watchtower), and `signal_hub` (per-process WebSocket signaling registry); shared across both listeners
- `gamemode.rs` — server-authoritative `bounds_for(GameMode) -> (u8, u8)` and `validate_player_count` helper. Exhaustive `match` with no wildcard, so adding a future `GameMode` variant in `shared/` without a bounds row here is a compile error
- `api/` — HTTP route handlers (game listener only)
  - `register.rs` — `POST /register` — player identity issuance
  - `host.rs` — `POST /host` — session creation; validates `gamemode` (typed) and `player_count` against the gamemode's bounds before allocating a session code
  - `join.rs` — `POST /join` — atomic Redis-Lua append into the joiners list; rejects full sessions with `SessionFull`, host with `cannot_join_own_session`, duplicate joiner with `already_joined`
  - `session.rs` — `GET /session/{code}` and `DELETE /session/{code}`
  - `start.rs` — `POST /session/{code}/start` — transitions Waiting → Starting; preconditions: caller is host, status is Waiting, current count ≥ gamemode min, every member has a live WS in `SignalHub`; broadcasts `start_signaling` to the lobby
- `signaling/` — WebSocket signaling for WebRTC peer setup
  - `mod.rs` — `SignalHub` in-process registry of rooms (`code → player_id → mpsc::UnboundedSender<ServerMsg>`). `tokio::sync::RwLock` for concurrent broadcasts. Eager empty-room cleanup. Not Redis-backed: signaling state is ephemeral per-process
  - `protocol.rs` — `ClientMsg` (incoming) and `ServerMsg` (outgoing) JSON-tagged enums. Server attests `from` on relayed frames based on the authenticated WS connection — clients cannot forge a `from`
  - `ws.rs` — `GET /ws/session/{code}` upgrade handler. 5s identify-frame deadline, token + membership validation, `tokio::select!` pump loop, host-disconnect-during-Waiting tears the session down
- `testharness/` — same-origin HTML/JS WebRTC test harness at `GET /test/webrtc`, gated by `ENABLE_TEST_HARNESS=true` env var. Off by default. Vanilla JS, no build step
- `middleware/` — Tower middleware
  - `version.rs` — `X-Launcher-Version` and `X-Game-Version` enforcement (HTTP 426) on `/host`, `/join`, `/session/{code}/start`
- `admin/` — password-protected web admin panel (admin listener only)
  - `auth.rs` — login, logout, bcrypt session handling
  - `dashboard.rs` — dashboard display, config update handlers, and all update system handlers (check, apply, schedule, cancel, settings, rollback)
  - `templates.rs` — HTML page functions (no template engine dependency)
- `update/` — server self-update system
  - `github.rs` — GitHub Releases API version check; uses `semver` to compare against `env!("CARGO_PKG_VERSION")`; supports ETag conditional requests (`update:github_etag` in Redis) and optional `GITHUB_TOKEN` Bearer auth
  - `watchtower.rs` — Watchtower HTTP API client (triggers container restart; Watchtower runs with `WATCHTOWER_NO_PULL=true`, so it no longer pulls on its own)
  - `docker.rs` — bollard Docker client; two entry points: `pull_channel_image(channel)` used by the auto-apply path, and `retag_for_rollback(versioned_tag, channel)` used by the admin rollback handler. `IMAGE_REPO` is a hardcoded const for defense-in-depth.
  - `task.rs` — long-running tokio background task; drives auto-check intervals, apply intervals, and scheduled updates via `UpdateCommand` channel. Every apply path acquires `AppState::update_apply_lock` before triggering Watchtower.

**Planned (future milestones):**
- `relay/` — real-time message relay between players once P2P is established
- `session/` — in-game session state management (scores, host promotion, reconnection)
- `matchmaking/` — lobby and player matching logic

## Shared (`shared/`)
Rust library crate shared between `server/` and `launcher/`. No OS-specific built-ins.
The Godot client uses equivalent C# types defined in `client/scripts/`.
- `src/protocol/messages.rs` — request/response types for all server REST endpoints (`HostRequest`, `JoinRequest`, `JoinResponse`, `SessionPollResponse`, `CloseSessionRequest`, `StartSessionRequest`, plus the minimal `JoinedPeer` peer-descriptor)
- `src/types/gamemode.rs` — `GameMode` enum, the authoritative list of valid gamemode strings on the wire. Serde rejects unknown variants at deserialize time
- `src/types/player.rs` — `PlayerId` type with sequential formatting
- `src/types/session.rs` — `SessionStatus` enum (`Waiting`, `Starting`, `Active`, `Ended`)
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
- **Docker + Docker Compose** — server, Redis, and Watchtower run in containers for portable redeployment
- **Redis** — session storage, player registry, runtime config, and update system state with TTL auto-expiry; `appendonly yes` ensures all state survives restarts (persisted to the `./redis_data/` bind mount alongside the compose file)
- **Watchtower** — Docker sidecar that recreates/restarts the server container on command via its HTTP API. Runs with `WATCHTOWER_NO_PULL=true`, so it does **not** pull images itself — the server pre-pulls via `bollard` before triggering. Image pull failures appear in the server's logs (`docker compose logs server`), not Watchtower's.
- **Admin Panel** — password-protected web UI at `/admin` (admin port only) for managing runtime config (version gates, password, update settings) without container restarts
- **Systemd / Docker restart policies** — keeps containers alive across reboots
- **GitHub Actions** — CI on every push; versioned Docker image releases to GHCR on tag push (`v*`), with automatic channel detection (`stable` / `ea` / `dev`) from the tag format

## Tools (`tools/`)
- `build/` — build scripts and bundler configuration
- `dev/` — local dev helpers (hot reload, dev server, etc.)
