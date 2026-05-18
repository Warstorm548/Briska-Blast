# Testing Guide

Manual testing procedures for the Briska Blast server using `curl`.
Start the server with `docker compose up --build` before running any of these.

---

## Player Registration

Register two players (simulate host and joiner):

```bash
# Register player 1 (host)
curl -s -X POST http://localhost:25919/register | jq
# Response: { "player_id": "0000001", "secret_token": "..." }

# Register player 2 (joiner)
curl -s -X POST http://localhost:25919/register | jq
# Response: { "player_id": "0000002", "secret_token": "..." }
```

Store the returned `player_id` and `secret_token` values — you'll need them for the steps below.

---

## Full Session Flow (Host + Joiner)

Replace `PLAYER1_ID`, `TOKEN1`, `PLAYER2_ID`, `TOKEN2` with the values from registration.

**Step 1 — Host creates a session:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.1.0" \
  -d '{
    "player_id": "PLAYER1_ID",
    "secret_token": "TOKEN1",
    "external_ip": "1.2.3.4",
    "external_port": 54231
  }' | jq
# Response: { "session_code": "K9M2XP" }
```

**Step 2 — Host polls for a joiner (initially empty):**
```bash
curl -s http://localhost:25919/session/K9M2XP | jq
# Response: { "status": "waiting", "joiner_ip": null, "joiner_port": null }
```

**Step 3 — Joiner connects:**
```bash
curl -s -X POST http://localhost:25919/join \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.1.0" \
  -d '{
    "session_code": "K9M2XP",
    "player_id": "PLAYER2_ID",
    "secret_token": "TOKEN2",
    "external_ip": "5.6.7.8",
    "external_port": 34567
  }' | jq
# Response: { "host_ip": "1.2.3.4", "host_port": 54231 }
```

**Step 4 — Host polls again (now sees joiner):**
```bash
curl -s http://localhost:25919/session/K9M2XP | jq
# Response: { "status": "active", "joiner_ip": "5.6.7.8", "joiner_port": 34567 }
```
Both clients now have each other's external IP and port and can begin the simultaneous UDP hole-punch.

**Step 5 — Host tears down the session:**
```bash
curl -s -X DELETE http://localhost:25919/session/K9M2XP \
  -H "Content-Type: application/json" \
  -d '{"player_id": "PLAYER1_ID", "secret_token": "TOKEN1"}' \
  -w "%{http_code}"
# Response: 204
```

---

## Version Gate

**Test launcher version rejected:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.0.1" \
  -H "X-Game-Version: 0.1.0" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","external_ip":"1.2.3.4","external_port":54231}' | jq
# Response: { "error": "launcher_update_required", "minimum_version": "0.1.0", "current_version": "0.0.1" }
```

**Test game version rejected:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.0.1" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","external_ip":"1.2.3.4","external_port":54231}' | jq
# Response: { "error": "game_update_required", "minimum_version": "0.1.0", "current_version": "0.0.1" }
```

---

## Error Cases

**Wrong secret token → 401:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.1.0" \
  -d '{"player_id":"0000001","secret_token":"wrongtoken","external_ip":"1.2.3.4","external_port":54231}' | jq
# Response: { "error": "unauthorized" }
```

**Non-existent session code → 404:**
```bash
curl -s http://localhost:25919/session/XXXXXX | jq
# Response: { "error": "not_found" }
```

**Joining an already-active session → 409:**
```bash
# (run after step 3 above)
curl -s -X POST http://localhost:25919/join \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.1.0" \
  -d '{"session_code":"K9M2XP","player_id":"PLAYER2_ID","secret_token":"TOKEN2","external_ip":"5.6.7.8","external_port":34567}' | jq
# Response: { "error": "session_already_active" }
```

---

## Admin Panel

Visit `http://localhost:25920/admin` in a browser.

| Action | Expected result |
|---|---|
| Load `/admin` | Login page with "Briska Blast Admin Panel" heading |
| Submit wrong password | "Invalid password." message, stays on login page |
| Submit correct password (`@admin` default) | Redirected to dashboard |
| Dashboard loads with default password | Yellow warning banner shown |
| Update Min Launcher Version to `1.0.0` | Green success banner: "Launcher version set to 1.0.0" |
| Set Min Launcher Version to `notaversion` | Red error banner: "Invalid version format" |
| Change password (wrong current) | Red error banner: "Current password is incorrect" |
| Change password (passwords don't match) | Red error banner: "New passwords do not match" |
| Successful password change → logout → login with new password | Access granted |

**Server Updates section:**

| Action | Expected result |
|---|---|
| Dashboard loads | Updates section shows channel, version, and "Last checked: Never" initially |
| Press "Check for Updates" — no update available | `update:available_version` key absent in Redis; no update banner shown |
| Press "Check for Updates" — update available (mock by setting `SET update:available_version v9.9.9` in redis-cli) | Green banner shows "Update available: v9.9.9" with Apply Now and Schedule options |
| Press "Apply Now" | `update:previous_version` set in Redis; Watchtower API called |
| Schedule an update with a past datetime | Update applied immediately (delay is 0 or negative) |
| Schedule an update with a future datetime | `update:scheduled_at` set in Redis; scheduled datetime shown with Cancel button |
| Press Cancel on a scheduled update | `update:scheduled_at` and `update:scheduled_version` cleared; auto-update resumes if enabled |
| Enable Auto-update toggle | `update:auto_enabled = "true"` in Redis; interval dropdowns appear |
| Change check interval dropdown | `update:check_interval_secs` updated in Redis |
| Simulate rollback (set `SET update:previous_version v0.2.0` in redis-cli) | Rollback button appears showing "Rollback to v0.2.0" |
| Press Rollback | `update:auto_enabled` forced to `"false"`; `update:rollback_locked = "true"`; rollback locked notice appears |

**Verify version gate responds to admin changes:**
```bash
# 1. In admin panel (localhost:25920/admin), set Min Launcher Version to 1.0.0
# 2. Run this — should now be rejected:
curl -s -X POST http://localhost:25919/host \
  -H "X-Launcher-Version: 0.1.0" \
  -H "X-Game-Version: 0.1.0" \
  -H "Content-Type: application/json" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","external_ip":"1.2.3.4","external_port":54231}' | jq
# Response: { "error": "launcher_update_required", "minimum_version": "1.0.0", ... }
```

---

## Redis Inspection (via Portainer exec or local redis-cli)

```bash
redis-cli

# Check player counter
GET player:counter

# Check a player's token hash
GET player:0000001:token_hash

# Check a session
GET session:K9M2XP

# Check current version minimums
GET min_launcher_version
GET min_game_version

# Check admin password hash is set
EXISTS admin:password_hash

# Check update system state
GET update:current_version       # version the running binary reports
GET update:previous_version      # version before last update (rollback source)
GET update:auto_enabled          # "true" / "false"
GET update:check_interval_secs   # e.g. "21600"
GET update:apply_interval_secs   # e.g. "259200" or "" (immediate)
GET update:available_version     # latest version found on GitHub, if any
GET update:found_at              # unix timestamp when update was discovered
GET update:last_checked          # unix timestamp of last GitHub API poll
GET update:scheduled_at          # unix timestamp for pending manual schedule
GET update:scheduled_version     # version queued for scheduled apply
GET update:rollback_locked       # "true" when auto-update disabled after rollback
```
