# Server Changelog

All notable changes to the Briska Blast server are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
| `BIND_ADDR` | `0.0.0.0:8080` | Initial server bind address |
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
