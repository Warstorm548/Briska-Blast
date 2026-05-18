# Development Setup

## Prerequisites

| Tool | Purpose | Install |
|---|---|---|
| Docker + Docker Compose | Run server and Redis | [docs.docker.com](https://docs.docker.com/get-docker/) |
| Portainer | Web GUI for container management | Deployed as a Docker container |
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
WATCHTOWER_TOKEN=briska-watchtower-token
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
| `WATCHTOWER_PORT` | `25921` | Port for Watchtower's HTTP API. Follows the project's 25900s port range (prod: 25921, staging: 25931, dev: 25941). |
| `WATCHTOWER_TOKEN` | `briska-watchtower-token` | Shared secret between the server and Watchtower. Change this before deploying. |
| `WATCHTOWER_URL` | `http://watchtower:25921` | Internal Docker network address of Watchtower. Set automatically from `WATCHTOWER_PORT` in docker-compose. |

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
6. Configure the **Server Updates** section — set your release channel and optionally enable automatic updates

---

## Portainer Setup

Portainer is the recommended way to manage the running containers in production.

```bash
# Run Portainer (first time only)
docker volume create portainer_data
docker run -d \
  -p 9443:9443 \
  --name portainer \
  --restart=always \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v portainer_data:/data \
  portainer/portainer-ce:latest
```

Access Portainer at `https://yourserver:9443`. From there you can:
- Start, stop, and restart the server and Redis containers
- View live logs
- Change environment variables (requires container restart to apply)
- Exec into the Redis container for emergency Redis CLI access

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
