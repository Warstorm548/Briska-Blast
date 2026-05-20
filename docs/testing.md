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

**Behavioral note (v0.5.0):** peer connections are now WebRTC. The server stops storing peer IP/port (WebRTC discovers them client-side via STUN), and joining no longer flips status to `active` — the lobby stays `waiting` while it fills, then the host explicitly transitions it via `POST /session/{code}/start`. SDP/ICE exchange happens over a WebSocket at `/ws/session/{code}` — that part can't be exercised from curl. Use the test harness at `/test/webrtc` (start the server with `ENABLE_TEST_HARNESS=true`) for end-to-end WebRTC verification.

**Step 1 — Host creates a session:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{
    "player_id": "PLAYER1_ID",
    "secret_token": "TOKEN1",
    "gamemode": "extended",
    "player_count": 2
  }' | jq
# Response: { "session_code": "K9M2XP" }
```

`player_count` is the total lobby size including the host. Must fall within the gamemode's allowed range (`extended`: 2–4). Out-of-range requests are rejected before the session is created — see Error Cases.

**Step 2 — Host polls the session (initially host-only):**
```bash
curl -s http://localhost:25919/session/K9M2XP | jq
# Response:
# {
#   "status": "waiting",
#   "gamemode": "extended",
#   "player_count": 2,
#   "current_player_count": 1,
#   "joiner_player_ids": []
# }
```

**Step 3 — Joiner joins:**
```bash
curl -s -X POST http://localhost:25919/join \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{
    "session_code": "K9M2XP",
    "player_id": "PLAYER2_ID",
    "secret_token": "TOKEN2"
  }' | jq
# Response:
# {
#   "gamemode": "extended",
#   "player_count": 2,
#   "current_player_count": 2,
#   "joiners": [ { "player_id": "PLAYER2_ID" } ]
# }
```

**Step 4 — Host polls again (now sees the joiner; status is still waiting):**
```bash
curl -s http://localhost:25919/session/K9M2XP | jq
# Response:
# {
#   "status": "waiting",
#   "gamemode": "extended",
#   "player_count": 2,
#   "current_player_count": 2,
#   "joiner_player_ids": [ "PLAYER2_ID" ]
# }
```

**Step 5 — WebSocket signaling (not via curl).** Both players must connect to `ws://localhost:25919/ws/session/K9M2XP`, send `{"type":"identify","player_id":"…","secret_token":"…"}` as the first frame, then exchange offers/answers/ICE candidates. The test harness at `http://localhost:25919/test/webrtc` (requires `ENABLE_TEST_HARNESS=true` at startup) drives this from two browser tabs.

**Step 6 — Host starts the match:**
```bash
curl -s -X POST http://localhost:25919/session/K9M2XP/start \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id": "PLAYER1_ID", "secret_token": "TOKEN1"}' \
  -w "%{http_code}"
# Response: 204 — status transitions to "starting" and start_signaling is broadcast over the WS
```

Without an active WS for every session member, `/start` returns 409 `session_not_startable` with `reason: "not_all_peers_ready"`. Use the test harness if you want this to actually succeed.

**Step 7 — Host tears down the session:**
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
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","gamemode":"extended","player_count":2}' | jq
# Response: { "error": "launcher_update_required", "minimum_version": "<configured>", "current_version": "0.0.1" }
```

**Test game version rejected:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.0.1" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","gamemode":"extended","player_count":2}' | jq
# Response: { "error": "game_update_required", "minimum_version": "<configured>", "current_version": "0.0.1" }
```

---

## Error Cases

**Wrong secret token → 401:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id":"0000001","secret_token":"wrongtoken","gamemode":"extended","player_count":2}' | jq
# Response: { "error": "unauthorized" }
```

**Unknown gamemode → 400 (rejected at the deserialize boundary):**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id":"PLAYER1_ID","secret_token":"TOKEN1","gamemode":"banana","player_count":2}' | jq
# Response: 400 with serde's generic "Failed to deserialize the JSON body" — the typed
# GameMode enum rejects unknown variants before the handler runs.
```

**Out-of-range `player_count` → 400 invalid_player_count:**
```bash
curl -s -X POST http://localhost:25919/host \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id":"PLAYER1_ID","secret_token":"TOKEN1","gamemode":"extended","player_count":5}' | jq
# Response: { "error": "invalid_player_count", "min": 2, "max": 4, "requested": 5 }
```

**Non-existent session code → 404:**
```bash
curl -s http://localhost:25919/session/XXXXXX | jq
# Response: { "error": "not_found" }
```

**Joining a full session → 409 session_full:**
```bash
# Fill a player_count=2 session with one joiner first, then a third tries to join:
curl -s -X POST http://localhost:25919/join \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"session_code":"K9M2XP","player_id":"PLAYER3_ID","secret_token":"TOKEN3"}' | jq
# Response: { "error": "session_full", "capacity": 2 }
```

**Host trying to join their own session → 409 cannot_join_own_session.**

**Existing joiner trying to join again → 409 already_joined.**

**Starting a session by a non-host → 409 session_not_startable:**
```bash
curl -s -X POST http://localhost:25919/session/K9M2XP/start \
  -H "Content-Type: application/json" \
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -d '{"player_id":"PLAYER2_ID","secret_token":"TOKEN2"}' | jq
# Response: { "error": "session_not_startable", "reason": "not_host" }
```

**Other `session_not_startable` reasons:** `not_in_waiting` (already starting/active), `below_min_players` (lobby has fewer than the gamemode's min), `not_all_peers_ready` (a session member doesn't have a live WS — common when testing via curl).

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
| Press "Check for Updates" — update available | If the configured `RELEASE_CHANNEL` has a newer release on GitHub for the running version, redirect message reads "Update available: vX.Y.Z" and the banner appears with Apply Now and Schedule options. The handler now calls GitHub directly; pre-setting `update:available_version` via `redis-cli` will NOT cause a fake "update available" — only a real newer GitHub release will. |
| Press "Check for Updates" — second click within a few minutes | GitHub returns `304 Not Modified` (the cached `update:github_etag` matches). Redirect message reads "No changes since last check". Existing `update:available_version` / `update:found_at` preserved. |
| Press "Check for Updates" — GitHub unreachable | Redirect message reads "Check failed: could not reach GitHub". Existing `update:available_version` is preserved (transient network failure does not clear cached state). |
| Press "Apply Now" | `update:previous_version` set in Redis; the server pre-pulls the channel image via `bollard`, then calls the Watchtower API. Watch `docker compose logs server` for `pre-pulled ghcr.io/...:CHANNEL` — pull failures appear here as `tracing::warn!`, not in Watchtower's logs. |
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
  -H "X-Launcher-Version: 0.5.0" \
  -H "X-Game-Version: 0.5.0" \
  -H "Content-Type: application/json" \
  -d '{"player_id":"0000001","secret_token":"TOKEN1","gamemode":"extended","player_count":2}' | jq
# Response: { "error": "launcher_update_required", "minimum_version": "1.0.0", ... }
```

---

## Redis Inspection (via `docker exec` or local redis-cli)

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
GET update:apply_interval_secs   # e.g. "259200" or "0" (immediate)
GET update:available_version     # latest version found on GitHub, if any
GET update:found_at              # unix timestamp when update was discovered
GET update:last_checked          # unix timestamp of last GitHub API poll
GET update:scheduled_at          # unix timestamp for pending manual schedule
GET update:scheduled_version     # version queued for scheduled apply
GET update:rollback_locked       # "true" when auto-update disabled after rollback
GET update:github_etag           # last ETag from GitHub Releases API (set on 200; sent as If-None-Match)
```
