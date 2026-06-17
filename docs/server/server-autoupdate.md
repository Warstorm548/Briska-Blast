# Server Auto-Update System

> **About to change update-path code or `docker-compose.yml`?**
> Read [`changing-the-update-system.md`](changing-the-update-system.md) first. The self-update system has a different risk profile from the rest of the codebase: a bug in the update path can strand the server permanently.

## Overview

The BriskaBlast server supports self-managed updates via a combination of:

- **Compile-time release channels** — the binary knows its own channel at runtime
- **GitHub Releases** — source of truth for available versions
- **Watchtower** — Docker sidecar that recreates/restarts the server container. Runs with `WATCHTOWER_NO_PULL=true` (see [Watchtower](#watchtower) below), so it does not pull images itself; the server owns image pulls via `bollard`.
- **Admin panel** — operator UI for manual checks, scheduling, and rollback

---

## Release Channels

| Channel | Tag format | GitHub Release type |
|---|---|---|
| `stable` | `v1.2.3` | Full release |
| `ea` (Early Access) | `v1.2.3-ea.1` | Pre-release |
| `dev` | `v1.2.3-dev.1` | Pre-release |

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

Watchtower runs as a sidecar in `docker-compose.yml`. It restarts the server container when triggered. The image is **pinned to `containrrr/watchtower:1.7.1`** — never `:latest` — so an upstream Watchtower release cannot auto-deploy itself into the stack.

Watchtower is configured in **HTTP API-only mode** with `WATCHTOWER_NO_PULL=true`. This means:
- Watchtower never polls or pulls images on its own.
- The **server** owns image pulling, via `bollard` inside `update/docker.rs`:
  - The auto-apply path (`task.rs::ApplyNow` / `maybe_apply` / `wait_and_apply`) calls `pull_channel_image(channel)` before triggering Watchtower.
  - The rollback path (`admin/dashboard.rs::rollback_update`) calls `retag_for_rollback` which pulls the pinned versioned image and retags it locally.
- Watchtower's job is reduced to: compare the running container's image ID against the latest local image, recreate if different.

This split exists because, without `NO_PULL`, Watchtower would pull the registry's newer channel image on every trigger — silently undoing the rollback flow's local retag.

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

The port is published via `expose:` (Docker network only), **not** `ports:`. The Watchtower HTTP API is reachable from the server container on the internal compose network, but **not** from the host shell. If you need to test Watchtower's API directly during debugging, `docker exec` into a container on the same compose network.

### Shared secret

Set `WATCHTOWER_TOKEN` in `.env` to a strong random value. It must match between the server service and the Watchtower service. **This variable is required** — `docker-compose.yml` uses `${WATCHTOWER_TOKEN:?...}`, so `docker compose up` fails fast if the variable is missing. There is no default token in compose, by design: a missing `.env` must never silently run with a known-public secret.

```env
WATCHTOWER_TOKEN=your-secret-token-here
```

Generate one with `openssl rand -base64 32`.

### Optional: GitHub API token

The update check polls the public GitHub Releases API. Anonymous calls share a 60 req/hr/IP rate-limit budget with everything else on the host. Setting `GITHUB_TOKEN` in `.env` raises that to 5000 req/hr authenticated — useful if you click "Check for Updates" frequently or run multiple environments behind the same egress IP. Any classic PAT with no scopes (or a read-only fine-grained token) works.

```env
GITHUB_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxx
```

Absence is fully supported — the server falls back to anonymous calls. The check also uses ETag conditional requests (`If-None-Match`) so unchanged responses (`304 Not Modified`) cost nothing against the rate limit either way.

Two caveats worth knowing: the variable only reaches the container if the `server` service in `docker-compose.yml` passes it through (`- GITHUB_TOKEN=${GITHUB_TOKEN:-}` under `environment:`), and because that's a host-side compose file — **not** part of the server image — it never arrives via an image pull, version tag, or Watchtower update; you update it on the host directly. For multi-environment setup on one dedi (a token per env, refreshing an env's compose without git), see the `GITHUB_TOKEN` note in the [ops manual](briska-blast-ops-manual.md#compose-stacks).

---

## Admin Panel — Server Updates Section

The **Server Updates** section appears in the admin dashboard below Version Control.

### Always available
- **Channel / Version / Last checked** — displayed at all times
- **Check for Updates** button — queries the GitHub Releases API immediately and updates the displayed status. While running, the automatic schedule is paused. The button has three possible outcomes:
  - *"Update available: vX.Y.Z"* — a newer matching release was found
  - *"Already up to date"* — GitHub returned 200 OK and no candidate was newer than the running version
  - *"No changes since last check"* — GitHub returned 304 Not Modified (the ETag matched); cached state preserved

### When an update is found (manual check)
- Shows the available version
- **Apply Now** — pre-pulls the new channel image via the server's Docker socket, then triggers Watchtower to restart the container. Returns "Update triggered" immediately even though pulling continues in the background.
- **Schedule** — pick a date/time via the datetime picker; the update applies automatically at that time. The scheduled time is shown with a **Cancel** button.

### Automatic Updates toggle
When enabled, two options appear:
- **Check every** — how often the server polls GitHub Releases (6h / 12h / 24h / 48h)
- **Apply after** — how long after an update is found before it is automatically applied (Immediately / 1 day / 3 days / 1 week / 2 weeks)

When auto-update finds a version and the apply interval has elapsed, the server pre-pulls the new image and triggers Watchtower automatically with no prompt.

### Rollback
When a previous version is stored (recorded automatically after each
successful apply), a **Rollback** button appears showing the previous version.

Pressing it:
1. Validates the submitted version parses as semver (as of v0.4.2)
2. Cross-checks it against `update:previous_version` in Redis — mismatch is
   rejected with an error (as of v0.4.2). The hidden form field is cosmetic;
   Redis is the source of truth, so a tampered POST cannot deploy an
   arbitrary historical tag.
3. Pulls the pinned versioned image (e.g. `:v0.3.0`) from GHCR via `bollard`
4. Retags it locally as the channel tag (e.g. `:stable`)
5. Triggers Watchtower to restart with the retagged image
6. **Disables auto-update** as a safety lock (`update:rollback_locked="true"`,
   `update:auto_enabled="false"`) — the toggle must be manually re-enabled
   once the rolled-back version is confirmed stable. As of v0.4.2, the apply
   paths additionally re-check `rollback_locked` *after* acquiring the apply
   mutex, so an auto-apply that was already queued cannot silently overwrite
   a fresh rollback.

### Apply-path serialisation

All paths that trigger Watchtower (`Apply Now`, scheduled apply, timer auto-apply, and rollback) acquire a single in-process `tokio::sync::Mutex` before firing. Watchtower itself is idempotent, but the Redis writes around the trigger (`update:previous_version`, `update:available_version`, `update:found_at`) must not interleave. A rollback request submitted while an auto-apply is mid-flight will briefly wait for the auto-apply's mutex guard to drop before proceeding.

As of v0.4.2, the apply paths follow a **lock-then-read** discipline:
authoritative state (`auto_enabled`, `rollback_locked`, `scheduled_at`,
`available_version`) is re-read from Redis *after* the mutex has been
acquired, and the decision to call `watchtower::trigger_update` is made on
that fresh read. This closes a race where a rollback could complete during
the window between an auto-apply's state read and its lock acquisition — the
auto-apply would otherwise resume and silently re-overwrite the rollback.

### Where update failures appear

Because the server now owns image pulling, pull failures (network blip to GHCR, registry rate-limit, disk full) surface as `tracing::warn!` entries in the server's logs, not in Watchtower's logs. If "Apply Now" reports success but the running version doesn't change, the first thing to check is:

```bash
docker compose logs server | grep -i "pre-pull failed"
```

---

## Redis Keys Reference

All update state is stored in Redis (persisted to disk via the `./redis_data/` bind mount alongside the compose file) and survives container restarts and updates.

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
| `update:rollback_locked` | `"true"` after a rollback — auto-update blocked until manually cleared |
| `update:github_etag` | Last ETag returned by the GitHub Releases API — sent back as `If-None-Match` on subsequent polls so unchanged responses cost zero rate-limit quota |

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
git tag v1.2.3-dev.1  # dev (pre-release)
git push origin <tag>
```
