# Briska-Blast Server Operations Manual

*A practical reference for managing the Briska-Blast matchmaking server stack on a dedicated server.*

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Port Allocation Strategy](#2-port-allocation-strategy)
3. [Directory Layout on the Dedi](#3-directory-layout-on-the-dedi)
4. [Per-Environment nginx Configuration](#4-per-environment-nginx-configuration)
5. [Standard Operations](#5-standard-operations)
6. [Security Notes](#6-security-notes)
7. [Troubleshooting](#7-troubleshooting)
8. [Quick Reference: Common Commands](#8-quick-reference-common-commands)
9. [Key Principles to Remember](#9-key-principles-to-remember)

---

## 1. Architecture Overview

Briska-Blast runs as a multi-tier stack on a dedicated server (the "dedi"):

```
Player's launcher
       │  HTTPS to api.briska-blast.com
       ▼
DNS resolution → dedi public IP
       │  TCP to :443
       ▼
nginx (bare-metal on host)
       │  HTTP to 127.0.0.1:GAME_PORT
       │       or 127.0.0.1:ADMIN_PORT
       ▼
Axum server (in Docker container)
       │  internal Docker network
       ▼
Redis (in Docker container)
```

### Key Architectural Facts

- **nginx runs bare-metal on the host** (shared with the Pterodactyl panel).
- **Axum runs in Docker** with two listeners: one for game endpoints, one for admin endpoints.
- **Redis runs in Docker** alongside Axum, internal to the compose network only.
- **Multiple environments** (prod / staging / dev) run as separate compose stacks on the same host.
- **nginx is the only public-facing service** — Axum is reachable only via loopback (127.0.0.1) from the host.

---

## 2. Port Allocation Strategy

### The Allocation Table

| Environment | Game Port | Admin Port |
|-------------|-----------|------------|
| Prod        | 25919     | 25920      |
| Staging     | 25929     | 25930      |
| Dev         | 25939     | 25940      |

### The Tens-Digit Pattern

Ports are allocated in per-environment pairs, with game/admin separated by +1:

- Prod: 25919 (game), 25920 (admin)
- Staging: 25929 (game), 25930 (admin)
- Dev: 25939 (game), 25940 (admin)

This keeps each environment in its own numeric block and makes environment ownership easy to spot in logs/config at a glance.

### Why This Range?

Ports in the 25900s are:

- Inside the IANA registered range (1024–49151), but outside commonly used defaults
- Unlikely to conflict with standard services in most deployments
- Memorable as a block

If a port ever conflicts with another service on the dedi (a game-server panel, another project, etc.), pick the next free number in the same tens range. Example: if 25919 conflicts, try 25911 or 25917 — staying within the prod block so the tens digit still identifies the env.

---

## 3. Directory Layout on the Dedi

### Compose Stacks

Each environment lives in its own folder with its own compose file and `.env`:

```
~/briska/
├── prod/
│   ├── docker-compose.yml
│   └── .env              (GAME_PORT=25919, ADMIN_PORT=25920)
├── staging/
│   ├── docker-compose.yml
│   └── .env              (GAME_PORT=25929, ADMIN_PORT=25930)
└── dev/
    ├── docker-compose.yml
    └── .env              (GAME_PORT=25939, ADMIN_PORT=25940)
```

### nginx Configuration

Each environment has its own nginx config file:

```
/etc/nginx/sites-available/
├── briska-prod.conf
├── briska-staging.conf
└── briska-dev.conf
```

Each enabled via symlink in `sites-enabled/`:

```bash
sudo ln -s /etc/nginx/sites-available/briska-prod.conf /etc/nginx/sites-enabled/
```

### TLS Certificates

Managed by Certbot, stored in:

```
/etc/letsencrypt/live/
├── api.briska-blast.com/
│   ├── fullchain.pem
│   └── privkey.pem
├── staging.briska-blast.com/
└── dev.briska-blast.com/
```

Renewal is automatic via a systemd timer or cron job — no manual action needed in normal operation.

---

## 4. Per-Environment nginx Configuration

Below is the template for an environment's nginx config. Substitute the right values for each environment.

### Template: `briska-prod.conf`

```nginx
# /etc/nginx/sites-available/briska-prod.conf

server {
    listen 443 ssl http2;
    server_name api.briska-blast.com;

    ssl_certificate     /etc/letsencrypt/live/api.briska-blast.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.briska-blast.com/privkey.pem;

    # ---- Game API (public, used by launchers worldwide)
    location / {
        proxy_pass http://127.0.0.1:25919;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # ---- Admin panel (locked down)
    # Exact match for /admin (no trailing slash) — the login page route
    location = /admin {
        allow 203.0.113.42;   # replace with your home IP
        deny all;

        auth_basic "Briska Admin (prod)";
        auth_basic_user_file /etc/nginx/.htpasswd-prod;

        proxy_pass http://127.0.0.1:25920;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # Prefix match for all other admin routes (/admin/login, /admin/dashboard, etc.)
    location /admin/ {
        allow 203.0.113.42;   # replace with your home IP
        deny all;

        auth_basic "Briska Admin (prod)";
        auth_basic_user_file /etc/nginx/.htpasswd-prod;

        proxy_pass http://127.0.0.1:25920;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

# HTTP → HTTPS redirect (Certbot adds this automatically)
server {
    listen 80;
    server_name api.briska-blast.com;
    return 301 https://$host$request_uri;
}
```

### What Each Section Does

- **`server_name`** — which domain this server block handles.
- **`ssl_certificate*`** — TLS certs from Let's Encrypt via Certbot.
- **`location /`** — catches all requests EXCEPT those matching the admin blocks. Forwards to the game port.
- **`location = /admin`** — exact match for the bare `/admin` path (the login page). Without this, `/admin` with no trailing slash falls through to `location /` and hits the game port instead.
- **`location /admin/`** — prefix match for all other admin routes (`/admin/login`, `/admin/dashboard`, etc.). Three layers of protection on both admin blocks:
  1. IP allowlist (only specified IPs can reach the endpoint)
  2. HTTP basic auth (password required)
  3. Forwards to admin port where Axum applies its own auth
- **HTTP → HTTPS redirect block** — bumps unencrypted requests to HTTPS.

### Setting Up Basic Auth Password Files

```bash
# Create initial password file (per environment)
sudo htpasswd -c /etc/nginx/.htpasswd-prod adminuser

# Add additional users to an existing file
sudo htpasswd /etc/nginx/.htpasswd-prod anotheruser
```

The `-c` flag creates a new file (and overwrites any existing one). Omit `-c` when adding to an existing file.

---

## 5. Standard Operations

### Starting an Environment

```bash
cd ~/briska/prod
docker compose up -d
```

The `-d` flag runs in detached mode (background). Check status with:

```bash
docker compose ps
```

### Stopping an Environment

```bash
cd ~/briska/prod
docker compose down
```

This stops and removes the containers (volumes persist). For just stopping without removing:

```bash
docker compose stop
```

### Restarting After Code Changes

```bash
cd ~/briska/prod
docker compose pull       # if pulling from a registry
docker compose up -d      # recreates containers with new code
```

### Changing a Port (Manual Sync Procedure)

This is the full procedure whenever a port conflicts or you want to relocate a server.

**Example:** Change prod's game port from 25919 to 25927.

```bash
# 1. Edit the env's .env file to the new port
nano ~/briska/prod/.env
# Change:  GAME_PORT=25919  →  GAME_PORT=25927

# 2. Edit nginx's config for that env to match
sudo nano /etc/nginx/sites-available/briska-prod.conf
# Change:  proxy_pass http://127.0.0.1:25919;
# To:      proxy_pass http://127.0.0.1:25927;

# 3. Validate nginx config BEFORE reloading
sudo nginx -t

# 4. Recreate the server container so new env values are applied
cd ~/briska/prod
docker compose up -d --force-recreate server

# 5. Reload nginx so it picks up the new config
sudo nginx -s reload

# 6. Verify
curl http://127.0.0.1:25927/   # should reach the game server
```

**Then update the port allocation table** in this document so future-you can find the right number.

### Adding a New Environment

To add a new env (e.g., a "beta" branch):

1. Pick port numbers following the tens-digit pattern (beta could be 25949 / 25950).
2. Create directory: `mkdir -p ~/briska/beta`
3. Create `docker-compose.yml` and `.env` in that directory.
4. Add DNS A record: `beta.briska-blast.com` → dedi public IP.
5. Issue TLS cert: `sudo certbot --nginx -d beta.briska-blast.com`
6. Create nginx config at `/etc/nginx/sites-available/briska-beta.conf` (copy from prod, adjust ports/cert path/server_name).
7. Enable: `sudo ln -s /etc/nginx/sites-available/briska-beta.conf /etc/nginx/sites-enabled/`
8. Create basic auth file: `sudo htpasswd -c /etc/nginx/.htpasswd-beta adminuser`
9. Validate and reload: `sudo nginx -t && sudo nginx -s reload`
10. Start the stack: `cd ~/briska/beta && docker compose up -d`
11. Update the port allocation table.

### After Host Reboot

Compose stacks with `restart: always` should auto-start. nginx should auto-start via systemd. Verify:

```bash
# Check compose stacks are running
docker ps

# Check nginx is running
sudo systemctl status nginx
```

If something didn't start:

```bash
# Start each env that didn't come up
cd ~/briska/prod && docker compose up -d

# Start nginx if needed
sudo systemctl start nginx
```

---

## 6. Security Notes

### Loopback Binding (BIND_ADDR)

By default, `.env` does NOT set `BIND_ADDR`, which means Docker port mappings default to `127.0.0.1`. This makes the Axum ports reachable ONLY from the host (i.e., from nginx).

**Never set `BIND_ADDR=0.0.0.0` in production.** That binds the Axum ports to all network interfaces, making them reachable from the public internet — which bypasses:

- TLS encryption (nginx terminates TLS, Axum speaks plain HTTP)
- nginx IP allowlists
- nginx rate limiting
- nginx basic auth

`BIND_ADDR=0.0.0.0` is acceptable ONLY for trusted local development on a machine that isn't internet-reachable.

### Admin Endpoint Protection Layers

Admin endpoints are protected by THREE independent layers, working defense-in-depth:

| Layer | Where it lives | What it does |
|-------|----------------|--------------|
| 1. IP allowlist | nginx `location /admin/` | Only specified IPs can even reach the endpoint |
| 2. HTTP basic auth | nginx `location /admin/` | Browser/client must provide username + password |
| 3. App-level auth | Axum admin handlers | Internal authentication (e.g., `ADMIN_PASSWORD`) |

If any one layer is breached, the others still protect.

**Important — don't lock yourself out.** Keep at least one "break glass" entry in the allowlist (e.g., a VPN endpoint or a trusted external IP) so a changed home IP doesn't cut you off from admin entirely.

### TLS Cert Management

Certbot handles cert renewal automatically. Renewal checks run twice daily; certs renew when they have 30 days or less remaining.

**Verify renewal is configured:**

```bash
sudo systemctl list-timers | grep certbot
# OR
sudo systemctl status certbot.timer
```

**Manually test renewal:**

```bash
sudo certbot renew --dry-run
```

If renewal fails, Certbot logs warnings. Check periodically:

```bash
sudo journalctl -u certbot
```

### Admin Password

The `ADMIN_PASSWORD` env var is the app-level (third-layer) auth. Change the default before deploying. Generate a long random value:

```bash
openssl rand -base64 32
```

Set it in `.env` rather than hardcoding in `docker-compose.yml`, so it doesn't accidentally end up in version control.

### Future Hardening: Passkey Auth via Pocket ID

Pocket ID is a self-hosted OIDC provider supporting passkey-only authentication. Integrating it would replace `ADMIN_PASSWORD` as the third-layer auth with phishing-resistant, origin-bound passkeys. Treat this as a planned enhancement — not currently implemented.

---

## 7. Troubleshooting

### `sudo nginx -t` Fails

The validator caught a syntax error. Read the error message — it names the file and line number. Common causes:

- Missing semicolon at end of a directive
- Mismatched braces `{` `}`
- Typo in a directive name
- Cert file path doesn't exist

Fix the issue, run `nginx -t` again until it passes, THEN reload.

**Never run `sudo nginx -s reload` while `-t` still fails** — it could leave nginx in a broken state.

### Service Unreachable from Internet (502 Bad Gateway)

This usually means nginx is running but can't reach the backend. Check in order:

```bash
# 1. Is the Axum container running?
cd ~/briska/prod
docker compose ps

# 2. Is it listening on the right port?
sudo ss -tlnp | grep 127.0.0.1:25919

# 3. Does the port in .env match the port in nginx config?
grep GAME_PORT ~/briska/prod/.env
grep proxy_pass /etc/nginx/sites-available/briska-prod.conf
```

If `.env` and nginx config disagree on the port, that's the bug — finish the manual sync procedure.

### Port Already in Use

If `docker compose up` fails with "bind: address already in use":

```bash
# Find what's using the port
sudo ss -tlnp | grep :25919
# Or with lsof:
sudo lsof -i :25919
```

If it's another service you control, decide whether to stop it or move your env to a different port (follow the port-change procedure).

### Certificate Renewal Failed

If browsers warn about expired certs:

```bash
# Force renewal attempt
sudo certbot renew --force-renewal

# If that fails, check why
sudo journalctl -u certbot --since "24 hours ago"
```

Common causes:

- DNS records changed (cert won't issue if domain doesn't resolve to your dedi)
- Port 80 blocked (Certbot's HTTP-01 challenge uses port 80)
- Rate limits hit (Let's Encrypt limits issuance per domain)

### Lost Admin Access (Locked Out by IP Allowlist)

If your IP changed and you can't reach `/admin/`:

```bash
# SSH to dedi, edit nginx config to add your new IP
sudo nano /etc/nginx/sites-available/briska-prod.conf
# Add: allow YOUR.NEW.IP.HERE;

# Validate and reload
sudo nginx -t
sudo nginx -s reload
```

### Compose Stack Won't Start

```bash
# Look at the actual error
cd ~/briska/prod
docker compose up    # no -d, so logs print to your terminal

# Or check logs of the failed container
docker compose logs server
```

Common causes:

- Port conflict (see "Port Already in Use")
- Bad env var in `.env` (compose validates these strictly)
- Image build failure (try `docker compose build --no-cache`)

---

## 8. Quick Reference: Common Commands

### nginx

```bash
sudo nginx -t                            # Validate config
sudo nginx -s reload                     # Reload config (no downtime)
sudo systemctl restart nginx             # Hard restart (brief downtime)
sudo systemctl status nginx              # Check status
sudo journalctl -u nginx -f              # Live log tail
```

### Docker Compose (run from env directory)

```bash
docker compose up -d                     # Start in background
docker compose down                      # Stop and remove containers
docker compose restart server            # Restart just the server service
docker compose ps                        # Show containers
docker compose logs -f server            # Tail server logs
docker compose pull                      # Pull updated images
docker compose build --no-cache          # Force full rebuild
```

### Cert Management

```bash
sudo certbot certificates                # List managed certs
sudo certbot renew --dry-run             # Test renewal
sudo certbot renew                       # Actual renewal (only renews if needed)
sudo certbot --nginx -d new.domain.com   # Issue cert for a new domain
```

### Diagnostics

```bash
sudo ss -tlnp                            # List all listening TCP ports
sudo ss -tlnp | grep 127.0.0.1           # Loopback-bound ports only
curl -v http://127.0.0.1:25919/          # Test backend directly (skips nginx)
curl -v https://api.briska-blast.com/    # Test through nginx end-to-end
docker ps                                # All running containers
docker logs <container-id>               # Specific container's logs
```

---

## 9. Key Principles to Remember

1. **Port numbers are a contract.** nginx, Docker, and Axum must all agree. If you change one, change them all (manual sync procedure).
2. **`.env` is the source of truth.** Don't hardcode ports in multiple places — read them from `.env` via environment-variable substitution.
3. **Always `nginx -t` before reload.** Free, instant, catches typos before they break the proxy.
4. **Loopback binding is non-negotiable in production.** `BIND_ADDR=127.0.0.1` is the only safe default. Public Axum ports skip every nginx-layer protection.
5. **Update the port allocation table immediately after any change.** Future-you will thank you.
6. **Admin panel controls *what the app does*, not *how it's exposed.*** Bind addresses, TLS, firewall rules — those are operator (shell access) concerns, not admin-panel concerns.
7. **Defense in depth.** Multiple security layers (IP allowlist + basic auth + app password) mean a breach of one doesn't compromise the whole system.
8. **Validate before applying.** `nginx -t`, `certbot --dry-run`, `docker compose config` — all of these check before doing.

---

*End of document.*
