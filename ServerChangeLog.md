# Server Changelog

All notable changes to the Briska Blast server are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [0.4.0] — 2026-05-18

### Added

**Server Auto-Update System**
- **Compile-time release channel** — `server/build.rs` reads `RELEASE_CHANNEL` at build time and bakes it into the binary. Accessible at runtime via `env!("RELEASE_CHANNEL")`. Defaults to `dev`; CI/CD sets `stable`, `ea`, or `dev` based on the release tag format.
- **`update/` module** — self-contained update subsystem:
  - `github.rs` — queries GitHub Releases API to detect newer versions for the binary's channel. Uses `semver` crate to compare against `env!("CARGO_PKG_VERSION")`; returns the latest matching tag if a newer version exists.
  - `watchtower.rs` — HTTP client for Watchtower's update API (`POST /v1/update`). Triggers Watchtower to pull the latest channel image and restart the server container.
  - `docker.rs` — uses `bollard` (Rust Docker client) to pull a pinned versioned image (e.g. `ghcr.io/warstorm548/briska-blast:v0.3.0`) and retag it as the channel tag. Used exclusively by the rollback flow.
  - `task.rs` — long-running Tokio background task spawned at startup. Drives all update scheduling logic via an `UpdateCommand` mpsc channel: periodic auto-checks, apply interval tracking, scheduled apply, and cancel.
- **Admin panel — Server Updates section** (six new routes on the admin listener):
  - `POST /admin/update/check` — manual on-demand check against GitHub Releases API; sets `update:manual_override` to suppress the auto-schedule while running
  - `POST /admin/update/apply-now` — immediately triggers Watchtower; stores current version as `update:previous_version` before applying
  - `POST /admin/update/schedule` — schedules update for a specific datetime (HTML `datetime-local` input); stores `update:scheduled_at` and `update:scheduled_version` in Redis
  - `POST /admin/update/cancel` — cancels a pending manual schedule; clears Redis keys; auto-update resumes if enabled
  - `POST /admin/update/settings` — saves auto-update toggle, check interval, and apply interval to Redis; notifies background task via `SettingsChanged`
  - `POST /admin/update/rollback` — pulls the pinned previous-version image via bollard, retags it as the channel tag, triggers Watchtower; forces `update:auto_enabled = false` and sets `update:rollback_locked = true` as a safety lock to prevent an auto-update re-applying the same version immediately after rollback
- **Update UI in admin dashboard** — new "Server Updates" section displaying channel, version, last-checked timestamp, available update banner with Apply Now / Schedule options, scheduled update display with Cancel button, rollback button (shown when a previous version is stored), rollback locked notice, and auto-update toggle with check interval and apply interval dropdowns
- **Watchtower sidecar** added to `docker-compose.yml` — runs in HTTP API-only mode (`--http-api-periodic-polls`); the server controls all polling and apply logic; Watchtower only executes the pull + restart
- **Docker socket mount** added to server service in `docker-compose.yml` — required for bollard rollback operations

### Changed

- `AppState` gains `update_tx: Arc<mpsc::Sender<UpdateCommand>>` — wired to the background update task at startup
- `Config` gains `watchtower_url` (`WATCHTOWER_URL`, default `http://watchtower:25921`) and `watchtower_token` (`WATCHTOWER_TOKEN`, default `briska-watchtower-token`)
- `server/Dockerfile` gains `ARG RELEASE_CHANNEL=dev` — passed as a build arg so `build.rs` stamps the channel correctly in image builds
- `docker-compose.yml` Watchtower port uses `${WATCHTOWER_PORT:-25921}` — follows the project's 25900s port allocation strategy rather than the conflicting default 8080; host-side binding is loopback-only (`127.0.0.1`)
- GitHub Actions `ci-server.yml` rewritten — was referencing Go 1.22 (stale); now runs `cargo build -p server` and `cargo test -p server` on Rust stable, triggered on pushes and PRs to `main`, `dev`, and `feature/**` when server or shared code changes
- GitHub Actions `release-server.yml` rewritten — was referencing Go 1.22 and disabled; now triggers automatically on `v*` tags, detects channel from tag format (`v1.2.3` → stable, `-ea` → ea, `-dev` → dev), builds Docker image via buildx with correct `RELEASE_CHANNEL` baked in, pushes both a versioned tag and a channel tag to GHCR, creates a GitHub Release (full for stable, pre-release for ea/dev)
- `.env.example` documents `RELEASE_CHANNEL`, `WATCHTOWER_PORT`, and `WATCHTOWER_TOKEN`

### New Redis Keys

| Key | Purpose |
|---|---|
| `update:current_version` | Version the running binary reports; set on startup |
| `update:previous_version` | Version before the last update; source for rollback button |
| `update:auto_enabled` | `"true"` / `"false"` — auto-update toggle state |
| `update:check_interval_secs` | How often to poll GitHub (e.g. `"21600"` = 6 hours) |
| `update:apply_interval_secs` | Delay before auto-applying a found update; `"0"` or empty = immediate |
| `update:available_version` | Latest version tag found on GitHub for the current channel |
| `update:found_at` | Unix timestamp when the available update was first discovered |
| `update:last_checked` | Unix timestamp of the last GitHub Releases API poll |
| `update:scheduled_at` | Unix timestamp for a pending manually scheduled update |
| `update:scheduled_version` | Version queued for the scheduled apply |
| `update:rollback_locked` | `"true"` after a rollback; auto-update blocked until manually cleared |

### New Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `reqwest` | 0.12 | HTTP calls to GitHub Releases API and Watchtower |
| `chrono` | 0.4 | Timestamp formatting and datetime-local parsing |
| `bollard` | 0.17 | Docker Engine API client for rollback image pull + retag |
| `futures-util` | 0.3 | Stream extension trait for bollard image pull stream |

### Configuration

| Environment Variable | Default | Purpose |
|---|---|---|
| `RELEASE_CHANNEL` | `dev` | Release channel baked at compile time: `stable`, `ea`, or `dev` |
| `WATCHTOWER_URL` | `http://watchtower:25921` | Internal Docker network address of the Watchtower service |
| `WATCHTOWER_TOKEN` | `briska-watchtower-token` | Shared secret for Watchtower HTTP API authentication |
| `WATCHTOWER_PORT` | `25921` | Host-side port for Watchtower HTTP API (prod: 25921, staging: 25931, dev: 25941) |

---

## [0.3.0] — 2026-05-16

### Added

- **`gamemode` field on sessions** — host sends `gamemode` in `POST /host`; server stores it in the Redis session object. Joiner receives `gamemode` in the `POST /join` response so the game client knows which mode to load, staying in sync with the host.
  - `HostRequest` gains `gamemode: String`
  - `Session` (Redis) gains `gamemode: String`
  - `JoinResponse` gains `gamemode: String`

---

## [0.2.0] — 2026-05-16

### Added

- **Dual-port listeners** — game and admin endpoints now run as two independent Axum `TcpListener` instances inside the same process, sharing `AppState` and a broadcast-channel graceful shutdown
  - `GAME_PORT` (default `25919`) — serves all player-facing endpoints: `/register`, `/host`, `/join`, `/session/{code}`
  - `ADMIN_PORT` (default `25920`) — serves all `/admin/*` endpoints exclusively
  - Requests to `/admin/*` on the game port return 404; requests to game endpoints on the admin port return 404 — route surfaces are physically separated
- **Startup port logs** — server logs `INFO game listener bound to 0.0.0.0:{port}` and `INFO admin listener bound to 0.0.0.0:{port}` at startup
- **Actionable bind-error messages** — once the process starts, if either in-process listener bind fails, the server logs the port, the error, and the env var to change (`GAME_PORT` or `ADMIN_PORT`), then exits non-zero
- **Graceful shutdown on both listeners** — `SIGTERM` and Ctrl+C stop both listeners cleanly via a `tokio::sync::broadcast` channel (a single watcher task broadcasts to both servers so neither misses the signal)
- **Server Ports section in admin dashboard** — read-only display of the game port and admin port the process started on, replacing the old runtime bind-address form
- **`.env.example`** — template at repo root documenting `BIND_ADDR`, `GAME_PORT`, and `ADMIN_PORT` overrides

### Changed

- Docker port mappings now default to loopback-only (`127.0.0.1`) so ports are unreachable from other machines without a reverse proxy. Set `BIND_ADDR=0.0.0.0` in `.env` to expose directly (trusted dev environments only).
- `docker-compose.yml` port entries parameterised: `${BIND_ADDR:-127.0.0.1}:${GAME_PORT:-25919}:${GAME_PORT:-25919}` and `${BIND_ADDR:-127.0.0.1}:${ADMIN_PORT:-25920}:${ADMIN_PORT:-25920}`
- `server/Dockerfile` `EXPOSE` updated from `8080` to `25919` and `25920`

### Removed

- **Runtime bind-address toggle** — the admin dashboard form for changing `server:bind_addr` and the `/admin/update/bind-addr` endpoint are removed. Bind address is now deployment-time configuration (compose / `.env`), not runtime configuration.
- `BIND_ADDR` environment variable removed from the container — Axum always binds `0.0.0.0` inside the container; host-side interface restriction is handled by Docker's port mapping.
- `server:bind_addr` Redis key is no longer seeded or read.

### Configuration

| Environment Variable | Default | Purpose |
|---|---|---|
| `REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `GAME_PORT` | `25919` | Port for all player-facing game endpoints |
| `ADMIN_PORT` | `25920` | Port for all `/admin/*` endpoints |
| `SESSION_TTL_SECS` | `1800` | Game session TTL in seconds |
| `MIN_LAUNCHER_VERSION` | `0.1.0` | Initial minimum launcher version (seeded to Redis on first boot) |
| `MIN_GAME_VERSION` | `0.1.0` | Initial minimum game version (seeded to Redis on first boot) |
| `ADMIN_PASSWORD` | `@admin` | Initial admin password (seeded to Redis on first boot as bcrypt hash) |
| `RUST_LOG` | `info` | Tracing log level |

> `BIND_ADDR` is a Docker Compose host-side variable (controls which host interface the ports are published on). It is **not** read by the server process.

---

## [0.1.0] — 2026-05-15

### Added

**Player Identity System**
- `POST /register` — first-contact endpoint that issues a sequential player ID (zero-padded, e.g. `0000001`) and a cryptographically random 32-byte secret token
- Player IDs are generated atomically via Redis `INCR` — no collisions under concurrent registration
- Secret token stored server-side as a SHA-256 hash; plaintext returned to client once for local storage
- Two-part identity (readable ID + secret token) used to authenticate reconnections and session actions

**Session Signaling (NAT Hole-Punch Brokering)**
- `POST /host` — host registers their STUN-resolved external IP and port, receives a 6-character session code to share with friends
- `POST /join` — joiner submits their external IP and port plus the session code; receives the host's endpoint in return; server stores joiner info in the session for the host to retrieve
- `GET /session/{code}` — host polls this to discover when a joiner has connected and retrieve their IP and port, enabling simultaneous UDP hole-punching from both sides
- `DELETE /session/{code}` — explicit session teardown; frees the code immediately rather than waiting for TTL expiry
- Session codes use a 31-character unambiguous alphabet (no `0 O 1 I L`) for easy verbal sharing
- Sessions stored in Redis with a 30-minute TTL; auto-expire on inactivity

**Version Gate**
- `X-Launcher-Version` header checked on `/host` and `/join` against `min_launcher_version` stored in Redis
- `X-Game-Version` header checked on `/host` and `/join` against `min_game_version` stored in Redis
- Returns HTTP `426 Upgrade Required` with `launcher_update_required` or `game_update_required` error identifying exactly which component is outdated
- Missing version headers treated as `0.0.0`; both minimums default to `0.1.0` and are runtime-configurable without redeploy
- Version comparison uses the `semver` crate — string comparison is never used

**Admin Panel**
- Password-protected web UI at `/admin`
- Login rate-limited to 5 attempts per 15 minutes per IP to block brute force
- Admin password set via `ADMIN_PASSWORD` environment variable; default first-install password is `@admin`
- Passwords stored as bcrypt hashes in Redis; never stored in plaintext
- Dashboard sections:
  - **Server Stats** — live count of active sessions and total registered players
  - **Version Control / Version Minimums to Join Game Sessions** — update `min_launcher_version` and `min_game_version` with immediate effect; no restart required
  - **Server Bind Address** — save a new bind address to Redis; applied on next container restart via Portainer
  - **Change Password** — verifies current password before accepting new one; enforces 6-character minimum
- Warning banner displayed on dashboard whenever the default `@admin` password is still in use
- Session tokens stored in Redis with 24-hour TTL; logout deletes the token immediately

**Infrastructure**
- Cargo workspace root (`server` + `shared` crates)
- `shared/` Rust library crate holds all request/response types and domain types shared between server and launcher
- Docker Compose stack: Axum server container + Redis container with `appendonly yes` for persistent player counter
- Multi-stage Dockerfile (build on `rust:1.77-slim`, run on `debian:bookworm-slim`)
- Per-IP rate limiting via `governor` on all endpoints
- Structured tracing via `tracing` + `tracing-subscriber`; log level controlled by `RUST_LOG` env var
- All runtime config (`min_launcher_version`, `min_game_version`, `server:bind_addr`, `admin:password_hash`) stored in Redis and changeable without code redeploy

### Configuration

| Environment Variable | Default | Purpose |
|---|---|---|
| `REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `BIND_ADDR` | `0.0.0.0:8080` | Server bind address (single listener, replaced in v0.2.0) |
| `SESSION_TTL_SECS` | `1800` | Game session TTL in seconds |
| `MIN_LAUNCHER_VERSION` | `0.1.0` | Initial minimum launcher version (seeded to Redis on first boot) |
| `MIN_GAME_VERSION` | `0.1.0` | Initial minimum game version (seeded to Redis on first boot) |
| `ADMIN_PASSWORD` | `@admin` | Initial admin password (seeded to Redis on first boot as bcrypt hash) |
| `RUST_LOG` | `info` | Tracing log level |

---

## [Unreleased]

- Relay logic for in-game ball physics packets
- Score validation (server-side trajectory checking)
- Session host promotion on disconnect
- Reconnection grace period handling
- Anti-cheat thresholds
