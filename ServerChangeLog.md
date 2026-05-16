# Server Changelog

All notable changes to the Briska Blast server are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [0.2.0] — 2026-05-16

### Added

- **Dual-port listeners** — game and admin endpoints now run as two independent Axum `TcpListener` instances inside the same process, sharing `AppState` and a broadcast-channel graceful shutdown
  - `GAME_PORT` (default `25919`) — serves all player-facing endpoints: `/register`, `/host`, `/join`, `/session/{code}`
  - `ADMIN_PORT` (default `25920`) — serves all `/admin/*` endpoints exclusively
  - Requests to `/admin/*` on the game port return 404; requests to game endpoints on the admin port return 404 — route surfaces are physically separated
- **Startup port logs** — server logs `INFO game listener bound to 0.0.0.0:{port}` and `INFO admin listener bound to 0.0.0.0:{port}` at startup
- **Actionable bind-error messages** — if either port is already in use, the server logs the port, the error, and the env var to change (`GAME_PORT` or `ADMIN_PORT`), then exits non-zero
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
