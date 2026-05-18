# Server Auto-Update System

## Overview

The BriskaBlast server supports self-managed updates via a combination of:

- **Compile-time release channels** — the binary knows its own channel at runtime
- **GitHub Releases** — source of truth for available versions
- **Watchtower** — Docker sidecar that performs the actual image pull and container restart
- **Admin panel** — operator UI for manual checks, scheduling, and rollback

---

## Release Channels

| Channel | Tag format | GitHub Release type |
|---|---|---|
| `stable` | `v1.2.3` | Full release |
| `ea` (Early Access) | `v1.2.3-ea.1` | Pre-release |
| `dev` (Experimental) | `v1.2.3-dev.1` | Pre-release |

The channel is baked into the binary at compile time via `server/build.rs` reading the `RELEASE_CHANNEL` environment variable. The GitHub Actions release workflow sets this automatically based on the tag format.

Each release pushes two Docker image tags to GHCR:
- `:stable` / `:ea` / `:dev` — always points to the latest for that channel
- `:v1.2.3` — pinned version tag, kept for rollback

---

## Configuring Which Channel to Run

Set `RELEASE_CHANNEL` in your `.env` file (or directly in `docker-compose.yml`):

```env
RELEASE_CHANNEL=stable
```

The `docker-compose.yml` passes this as a build arg so the binary is stamped correctly. The Docker image tag used should match the channel:

```yaml
# For stable:
image: ghcr.io/warstorm548/briska-blast:stable

# For early access:
image: ghcr.io/warstorm548/briska-blast:ea
```

---

## Watchtower

Watchtower runs as a sidecar in `docker-compose.yml`. It monitors the server container and performs the actual image pull and restart when triggered.

Watchtower is configured in **HTTP API-only mode** — it does not poll automatically. The server controls when Watchtower fires via the HTTP API.

### Port

Watchtower's HTTP API port follows the project's 25900s port allocation strategy — **not** the default 8080, which conflicts with common services on game server hosts (Pterodactyl, AMP, etc.).

Set `WATCHTOWER_PORT` in `.env` to match your environment's triplet:

| Environment | Watchtower Port |
|---|---|
| Prod | 25921 |
| Staging | 25931 |
| Dev | 25941 |

```env
WATCHTOWER_PORT=25921
```

### Shared secret

Set `WATCHTOWER_TOKEN` in `.env` to a strong random value. It must match between the server service and the watchtower service:

```env
WATCHTOWER_TOKEN=your-secret-token-here
```

---

## Admin Panel — Server Updates Section

The **Server Updates** section appears in the admin dashboard below Version Control.

### Always available
- **Channel / Version / Last checked** — displayed at all times
- **Check for Updates** button — queries the GitHub Releases API immediately and updates the displayed status. While running, the automatic schedule is paused.

### When an update is found (manual check)
- Shows the available version
- **Apply Now** — triggers Watchtower immediately (container restarts with new image)
- **Schedule** — pick a date/time via the datetime picker; the update applies automatically at that time. The scheduled time is shown with a **Cancel** button.

### Automatic Updates toggle
When enabled, two options appear:
- **Check every** — how often the server polls GitHub Releases (6h / 12h / 24h / 48h)
- **Apply after** — how long after an update is found before it is automatically applied (Immediately / 1 day / 3 days / 1 week / 2 weeks)

When auto-update finds a version and the apply interval has elapsed, Watchtower is triggered automatically with no prompt.

### Rollback
When a previous version is stored (set automatically before any update is applied), a **Rollback** button appears showing the previous version.

Pressing it:
1. Pulls the pinned versioned image (e.g. `:v0.3.0`) from GHCR
2. Retags it locally as the channel tag (e.g. `:stable`)
3. Triggers Watchtower to restart with the retagged image
4. **Disables auto-update** as a safety lock — the toggle must be manually re-enabled once the rolled-back version is confirmed stable

---

## Redis Keys Reference

All update state is stored in Redis (persisted to disk via the `redis_data` Docker volume) and survives container restarts and updates.

| Key | Description |
|---|---|
| `update:current_version` | Version the running binary reports (set on startup) |
| `update:previous_version` | Version before the last update (used for rollback button) |
| `update:auto_enabled` | `"true"` / `"false"` — auto-update toggle state |
| `update:check_interval_secs` | How often to poll GitHub (e.g. `"21600"`) |
| `update:apply_interval_secs` | Delay before auto-applying a found update (`"0"` = immediate) |
| `update:available_version` | Latest version tag found on GitHub (e.g. `"v0.4.0"`) |
| `update:found_at` | Unix timestamp when the available update was first discovered |
| `update:last_checked` | Unix timestamp of the last GitHub Releases API poll |
| `update:scheduled_at` | Unix timestamp for a pending manually scheduled update |
| `update:scheduled_version` | Version queued for the scheduled apply |
| `update:manual_override` | `"true"` while a manual check is in progress |
| `update:rollback_locked` | `"true"` after a rollback — auto-update blocked until manually cleared |

---

## GitHub Actions

### `ci-server.yml`
Triggers on pushes and PRs to `main`, `dev`, and `feature/**` branches when server or shared code changes. Runs `cargo build` and `cargo test` with `RELEASE_CHANNEL=dev`.

### `release-server.yml`
Triggers on version tags (`v*`). Detects the channel from the tag format, builds the Docker image with the correct `RELEASE_CHANNEL` baked in, pushes two tags to GHCR (channel tag + versioned tag), and creates a GitHub Release.

To release:
```bash
git tag v1.2.3        # stable
git tag v1.2.3-ea.1   # early access
git tag v1.2.3-dev.1  # dev/experimental
git push origin <tag>
```
