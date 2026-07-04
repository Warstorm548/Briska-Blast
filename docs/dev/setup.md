# Development Setup

## Prerequisites

| Tool | Purpose | Install |
|---|---|---|
| Docker + Docker Compose | Run server and Redis | [docs.docker.com](https://docs.docker.com/get-docker/) |
| Rust toolchain | Build server and launcher locally | `curl https://sh.rustup.rs -sSf \| sh` |
| Git | Source control | System package manager |

---

## Running the Server

### Option A — Docker Compose (recommended)

```bash
# From the project root
docker compose up --build
```

Game server starts on `http://localhost:25919`. Admin panel starts on `http://localhost:25920`. Redis and Watchtower start alongside both with persistence enabled.
On first boot the server seeds all default config values into Redis automatically.

To run in the background:
```bash
docker compose up --build -d
```

To stop:
```bash
docker compose down
```

---

### Option B — Local (without Docker)

Requires a running Redis instance on `localhost:6379`.

```bash
# Start Redis locally (if installed)
redis-server

# Build and run the server
cd server
cargo run
```

The server reads environment variables from a `.env` file in the project root if present.
Copy `.env.example` and edit as needed:

```
REDIS_URL=redis://127.0.0.1:6379
GAME_PORT=25919
ADMIN_PORT=25920
RUST_LOG=debug
ADMIN_PASSWORD=@admin
MIN_LAUNCHER_VERSION=0.1.0
MIN_GAME_VERSION=0.1.0
RELEASE_CHANNEL=dev
WATCHTOWER_PORT=25921
# REQUIRED — no default in docker-compose.yml. Generate with `openssl rand -base64 32`.
WATCHTOWER_TOKEN=replace-with-your-own-random-token
# Optional — raises GitHub Releases API rate limit from 60/hr anon to 5000/hr.
# GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxx
```

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `GAME_PORT` | `25919` | Port for all player-facing game endpoints |
| `ADMIN_PORT` | `25920` | Port for all `/admin/*` endpoints |
| `SESSION_TTL_SECS` | `1800` | How long a game session code lives in Redis (seconds) |
| `MIN_LAUNCHER_VERSION` | `0.1.0` | Minimum launcher version to host or join a session (seeded to Redis on first boot) |
| `MIN_GAME_VERSION` | `0.1.0` | Minimum game version to host or join a session (seeded to Redis on first boot) |
| `ADMIN_PASSWORD` | `@admin` | Initial admin panel password (seeded as bcrypt hash on first boot) |
| `RUST_LOG` | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |
| `RELEASE_CHANNEL` | `dev` | Release channel baked into the binary at compile time: `stable`, `ea`, or `dev`. Determines which GitHub Releases the update system monitors. |
| `WATCHTOWER_PORT` | `25921` | Internal Docker network port for Watchtower's HTTP API (published via `expose:`, not reachable from the host). Follows the 25900s port range (prod: 25921, staging: 25931, dev: 25941). |
| `WATCHTOWER_TOKEN` | **(required, no default)** | Shared secret between the server and Watchtower. `docker compose up` fails fast if missing. Generate with `openssl rand -base64 32`. |
| `WATCHTOWER_URL` | `http://watchtower:25921` | Internal Docker network address of Watchtower. Set automatically from `WATCHTOWER_PORT` in docker-compose. |
| `GITHUB_TOKEN` | *(unset)* | **Optional.** When set, the update check authenticates to the GitHub Releases API, raising the rate limit from 60 req/hr/IP to 5000 req/hr. Any classic PAT with no scopes works. |
| `TURN_KEY_ID` | *(unset)* | **Optional.** Cloudflare TURN key id (dashboard: Realtime → TURN keys). With both TURN vars set, the server mints short-lived TURN relay credentials for game clients at match start, so symmetric-NAT peer pairs can connect. Unset ⇒ TURN disabled (boot warn, STUN-only fallback). |
| `TURN_API_TOKEN` | *(unset)* | **Optional.** API token belonging to `TURN_KEY_ID`. Server-side only — clients never see it, only the minted short-lived credentials. |

> **Note:** `MIN_LAUNCHER_VERSION`, `MIN_GAME_VERSION`, and `ADMIN_PASSWORD` are only written
> to Redis on the very first boot (`SET NX`). After that, Redis is authoritative — use the
> admin panel to change them. To reset, delete the Redis key and restart the container.

> **Port binding:** Inside the container, Axum always binds `0.0.0.0`. The `BIND_ADDR` variable
> in `docker-compose.yml` controls which host-side interface the ports are published on
> (default: `127.0.0.1` — loopback only, safe behind a reverse proxy). Set `BIND_ADDR=0.0.0.0`
> in `.env` to expose ports directly (trusted dev environments only).

---

## First-Time Admin Setup

1. Start the server via Docker Compose
2. Visit `http://localhost:25920/admin` from the host machine, or `https://your-domain/admin` if nginx is configured as the public entrypoint
3. Log in with the default password: `@admin`
4. The dashboard will show a warning banner — **change the password immediately** using the Change Password section
5. Set your desired minimum versions for launcher and game
6. The **Server Updates** section shows the compile-time release channel of your binary — you cannot change it from the panel. To switch channels, redeploy with a different `RELEASE_CHANNEL` build arg. Use this section to enable automatic updates, set check/apply intervals, or trigger a manual update.

---

## Cargo Workspace

The project uses a Cargo workspace. To build or check all Rust crates at once:

```bash
# From project root
cargo check        # fast type-check, no binary output
cargo build        # debug build
cargo build --release  # optimised build
```

To target only the server:
```bash
cargo build -p server
```
