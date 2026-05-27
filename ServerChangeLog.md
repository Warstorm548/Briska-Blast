# Server Changelog

All notable changes to the Briska Blast server are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [0.7.0] — 2026-05-26

Server companions for Stage 1 of the multiplayer client — see
[`docs/planning/multiplayer-client-stages.md`](docs/planning/multiplayer-client-stages.md).
All three are small, focused additions that make a real lobby work.

### Added

- **Manual host transfer — `POST /session/:code/host`.** Lets the current
  host voluntarily hand the host role to a listed joiner, backing the
  lobby's "Promote" button. The read-validate-swap-write runs as a single
  Lua script (same atomicity as `/join` and `/start`): the demoted host
  re-enters `joiners` with a fresh timestamp (back of the join order), the
  new host moves out of `joiners`, and a `HostChanged` signaling frame is
  broadcast. Restricted to Waiting. Adds `TransferHostRequest` to `shared/`
  and `ServerMsg::HostChanged`. Route is version-gated, next to `/start`.
- **`host_player_id` in the `Identified` frame.** A joiner previously had
  no way to learn who the host is; the WS `Identified` reply now includes
  it so every client can render the host marker and anchor `HostChanged`.

### Changed

- **Free a joiner's slot on explicit leave (Waiting).** A joiner who sends
  a `Leave` frame while the session is Waiting is now removed from the
  Redis roster via an atomic `remove_joiner_if_waiting` script. Without
  this the slot stayed occupied until TTL, miscounting capacity and
  permanently blocking `/start` (its "all peers ready" check could never
  pass). A transient socket drop still keeps the slot for reconnect — only
  a deliberate leave frees it. `PeerLeft` now carries `reason` `"leave"`
  vs `"disconnect"`. (Guards the lua-cjson empty-table-as-`{}` pitfall so
  removing the last joiner still serializes `"joiners":[]`.)

## [0.6.0] — 2026-05-23

First cut of the per-user **dev_flag** pipeline that gates the launcher's
Dev-channel UI. Adds an admin "Users" tab to grant/revoke dev access,
widens the player_id space, and reshapes `/register` into the per-launch
identity-refresh endpoint the foundation doc has been planning.

### Added

- **`POST /register` is now idempotent** (`server/src/api/register.rs`).
  The launcher sends a `RegisterRequest { username, prior_player_id,
  prior_secret_token }` on every boot. When the prior creds match Redis,
  the server reuses the existing `player_id` and refreshes the stored
  username; when they don't match (corrupted launcher identity file) the
  server falls through to fresh issuance instead of 401-ing. The response
  now carries `username` (echo-back) and `dev_flag` (read from
  `player:<id>:dev_flag`, default `false`). Username trimmed and capped at
  32 chars; rate-limit unchanged (5/min per IP).

- **`POST /me/username`** (`server/src/api/me.rs`). Updates
  `player:<id>:username` after token validation via the existing
  `validate_player` helper. Returns `204 No Content`. New
  `rl_me_username` per-IP rate limiter mirrors `rl_register` (5/min). Used
  by the launcher's username change UI.

- **Admin `/admin/users` tab** (`server/src/admin/users.rs`,
  `server/src/admin/templates.rs`). New page lists every player by id +
  username with a Dev-access checkbox per row. Search bar filters by
  username or by id. A single Confirm-changes button submits a hidden
  `known_ids` field plus the visible checkboxes;
  `POST /admin/users/dev-flag` writes `player:<id>:dev_flag = "true"|"false"`
  per known id (refuses to write for ids with no token-hash record, so a
  tampered form can't manufacture players). Existing `Dashboard` ↔ `Users`
  nav is shared between both pages via a new `nav_html(active)` helper in
  `templates.rs`; CSS-only styling, no JS.

### Changed

- **`PlayerId::from_counter` width 7 → 9** in `shared/src/types/player.rs`.
  Newly issued ids are now zero-padded to 9 digits
  (`PlayerId::from_counter(42).to_string() == "000000042"`). Existing
  7-digit ids in Redis remain valid — they're stored as plain strings and
  match by hash, not by width. The admin Users tab numerically sorts ids
  so the two widths interleave by counter value rather than
  lexicographically.

- **Version** 0.5.1 → 0.6.0. Minor bump — the `/register` request body
  shape is incompatible with launcher versions prior to v0.4.0, which
  will fail at boot until they're updated. Aligns with the launcher
  v0.4.0 release.

### Notes

- Auth on `/admin/users*` reuses the existing `require_session()` cookie
  gate. CSRF protection / per-admin rate limiting are explicitly deferred
  — user direction: "will harden the routes for this later on this just a
  start."
- Game client / signaling paths are untouched. The dev_flag is consumed
  by the launcher UI only; server-side rejection of dev-channel routes
  based on the flag is a follow-up.

---

## [0.5.1] — 2026-05-20

Patch release — two targeted fixes to the auto-update flow surfaced
by the v0.5.0-dev.1 dev-server apply. No wire-format or behavioral
changes to game endpoints.

### Fixed

- **`update:previous_version` not advancing across applies
  (`server/src/update/task.rs`)** — observed on the
  v0.4.7 → v0.5.0-dev.1 apply: `current_version` advanced to `0.5.0`
  correctly, but `previous_version` remained at the stale `0.4.4`
  value left by older racy applies, leaving the Rollback button
  pointing at the wrong target.

  Root cause: the three apply call sites (`UpdateCommand::ApplyNow`,
  `maybe_apply`, `wait_and_apply`) did `pull` → `trigger_update` →
  `store_previous_version`. Watchtower's HTTP-API request kills the
  old container within roughly a second of accepting the trigger,
  and Watchtower's log timing confirms the SIGTERM lands before the
  post-trigger work has time to run — so step 3 never reaches Redis.

  **Fix**: write `update:previous_version` **before** triggering
  Watchtower. The pre-write is benign if `trigger_update` then fails
  (`previous_version` equals `current_version`, making Rollback a
  no-op — no worse than the old stale state). The `wait_and_apply`
  path still gates `clear_schedule` on `trigger_update` success so a
  failed apply leaves the schedule in place for retry.

- **Noisy `redis get scheduled_at failed` warnings
  (`server/src/update/task.rs`)** — when no schedule is queued
  (the common case), `update:scheduled_at` is absent from Redis.
  Three read sites annotated the result as `String`, so the absent
  key returned a nil that tripped redis-rs's "Response type not
  string compatible" type error and emitted a warning every
  startup and every auto-apply tick. Behavior was unaffected (the
  `unwrap_or_default()` catch turned it into "no schedule"), but
  the log was misleading.

  **Fix**: read as `Option<String>` in all three sites so a missing
  key returns `Ok(None)` cleanly. Same observable behavior, no
  more spurious warnings.

### Verification

The fix for the `previous_version` race is observable on the next
dev-channel apply: after `v0.5.0-dev.1 → v0.5.1-dev.1` lands,
`update:previous_version` should advance to `"0.5.0"` automatically
with no manual `redis-cli SET` intervention.

---

## [0.5.0] — 2026-05-20

Minor release (breaking, pre-1.0). Replaces the 2-player UDP
hole-punch design with N-player WebRTC signaling. The server stops
storing peer endpoints entirely — clients discover their own via
STUN, and the server's job narrows to validating session parameters
and relaying SDP / ICE between peers over a new WebSocket.

Adds a hardcoded server-authoritative per-gamemode `[min, max]`
player-count table that rejects out-of-spec session-creation requests
before any state is allocated. The 2-player join model is gone; a
`Session` now stores a list of joiners up to `player_count`, with the
race that previously allowed two concurrent joiners to both squeeze
past a full-cap check closed by doing the entire read-check-append
inside a single Redis Lua script. Lobby lifecycle gains an explicit
`Starting` phase — `/join` no longer side-effects status, only a new
`POST /session/:code/start` does. A WebSocket endpoint and per-process
`SignalHub` relay WebRTC signaling messages between peers; the server
attests every relayed `from` so impersonation isn't possible. A
same-origin HTML/JS test harness ships behind `ENABLE_TEST_HARNESS=true`
to verify the full flow without a Godot client.

Symmetric-NAT players (~5–10% of consumer routers) cannot participate
in this release because no TURN relay exists yet. TURN, the
host-configurable min/max-pair schema, and WS-ticket auth are tracked
in `docs/planning/roadmap.md`. All known edge cases live in the new
`docs/architecture/session-multiplayer-edge-cases.md`.

### Added

- **Typed `GameMode` enum (`shared/src/types/gamemode.rs`)** — initial
  variant: `Extended`. Serde rejects unknown gamemode strings at
  deserialize time, so any client sending an unrecognized gamemode is
  bounced at 400 before handler code runs. The matching server-side
  bounds module (`server/src/gamemode.rs`) uses an exhaustive `match`
  so adding a future variant without a bounds row is a compile error.
- **`SessionStatus::Starting` variant** — the lobby lifecycle splits
  from `Waiting → Active` into `Waiting → Starting → Active`.
- **`validate_player_count(mode, requested) → Result<(), AppError>`**
  in `server/src/gamemode.rs`. Returns the new
  `AppError::InvalidPlayerCount { min, max, requested }` (HTTP 400)
  carrying both the gamemode bounds and the offending value, so a
  client can render a useful message without a second round-trip.
- **`AppError::SessionFull { capacity }`** (HTTP 409) — returned by
  `/join` when the lobby is at its cap. Echoes the cap so the client
  doesn't have to poll to learn how full was full.
- **`AppError::SessionNotStartable { reason }`** (HTTP 409) — returned
  by `/session/:code/start` when preconditions fail. Stable reason
  discriminator (`not_host`, `not_in_waiting`, `below_min_players`,
  `not_all_peers_ready`).
- **N-player `Session` storage (`server/src/api/mod.rs`)** —
  `joiners: Vec<JoinerEntry>` replaces the single-joiner fields.
  `JoinerEntry { player_id, joined_at_ms }` persists the join
  timestamp for future kick-the-late-arrival logic. Helpers
  `current_player_count()`, `is_full()`, `contains_player()`.
- **`POST /session/:code/start`** (`server/src/api/start.rs`) —
  version-gated route. Preconditions: caller is host, status is
  `Waiting`, current count ≥ gamemode min, every session member has
  a live WS in `SignalHub`. Transitions status to `Starting` and
  persists to Redis BEFORE broadcasting `start_signaling`, so
  visible state and signaling intent stay aligned even if the
  broadcast fan-out is interrupted.
- **`server/src/signaling/` module** — `SignalHub` is an in-process
  registry of rooms keyed by session code, with `mpsc::UnboundedSender`
  per identified player. Not Redis-backed (signaling state is
  ephemeral, a server restart drops every WS anyway). `ServerMsg` and
  `ClientMsg` JSON-tagged enums cover the wire schema. Empty rooms
  are dropped eagerly in `leave_room` to prevent map leakage.
- **WebSocket endpoint `GET /ws/session/:code`** — handler upgrades
  the connection, requires `{"type":"identify",…}` as the first frame
  within 5s, validates the token via the existing `validate_player`
  helper, confirms the player is a member of the session, registers
  in `SignalHub`. `tokio::select!` pump loop over `socket.recv()` and
  `rx.recv()`. Server-attested `from` on every relayed offer / answer
  / ICE candidate. App-defined 4xxx close codes (4400 bad initial,
  4401 unauthorized, 4403 not in session, 4404 not found, 4500
  internal) mirror HTTP semantics. Host disconnect while session is
  still `Waiting` deletes the Redis session and broadcasts
  `session_ended { reason: "host_disconnect" }`.
- **Same-origin HTML/JS test harness at `GET /test/webrtc`** — gated
  by `ENABLE_TEST_HARNESS=true` env var (default off). Vanilla JS
  exercises register → host/join → identify → start → WebRTC mesh
  → data-channel broadcast. STUN at `stun.l.google.com:19302`. Glare
  avoidance via lexicographic `player_id`. WS auto-reconnect with
  exponential backoff for non-4xxx closes.

### Changed

- **`/host`** now validates `(gamemode, player_count)` against
  `gamemode::bounds_for` BEFORE generating a session code — invalid
  requests no longer waste a Redis collision-check round-trip.
- **`/join`** now does the entire read-check-append-write inside a
  Redis Lua script. Without this, two simultaneous joiners could
  both observe "1 slot left" and both succeed. The script returns
  a discriminated JSON envelope that the Rust side decodes via a
  typed enum.
- **`/join` no longer flips status to `Active`** — joining a session
  only fills a slot. The Waiting → Starting transition is now
  exclusively driven by `POST /session/:code/start`.
- **`/host`, `/join`, `/session/:code/start`** route to the
  version-gated subrouter (X-Launcher-Version + X-Game-Version
  enforced). `/ws/session/:code` does NOT — browsers cannot easily
  attach custom headers to a WS upgrade, and clients are already
  version-gated by the REST step they used to learn the code.

### Breaking

- **`HostRequest` drops `external_ip` and `external_port`** — peer
  endpoints are no longer the server's concern. WebRTC discovers
  them via STUN client-side.
- **`HostRequest.gamemode` is now `GameMode`** (typed) instead of a
  free-form `String`. Unknown values bounce at the deserialize
  boundary with HTTP 400.
- **`HostRequest` gains `player_count: u8`** — the host's chosen
  lobby cap, validated against the gamemode bounds. Includes the
  host (`player_count = 4` means 1 host + 3 joiners total).
- **`JoinRequest` drops `external_ip` and `external_port`.**
- **`JoinResponse` drops `host_ip` / `host_port`** and now carries
  `gamemode: GameMode`, `player_count: u8`, `current_player_count: u8`,
  and `joiners: Vec<JoinedPeer>`. A new joiner learns the full lobby
  roster from the join response — no separate poll needed.
- **`SessionPollResponse` switches to typed `status: SessionStatus`
  and `gamemode: GameMode`**, drops `joiner_ip` / `joiner_port`, and
  gains capacity + roster fields.
- **New `StartSessionRequest { player_id, secret_token }`** —
  session code comes from the URL path; only auth in the body.
- **New `JoinedPeer { player_id }`** — the minimal wire-side peer
  descriptor. Network addresses are not in HTTP responses anymore.

### Deferred (not in this release)

- TURN relay for symmetric-NAT players. Affected ~5–10% currently
  cannot participate; they see `kicked { reason: "peer_connection_unrecoverable" }`.
  See `docs/planning/roadmap.md`.
- Host-configurable `[min, max]` pair per gamemode (the host sends
  one value today; per-gamemode pair selection is a future shape).
- WS-ticket auth — `secret_token` is sent cleartext in the
  `Identify` frame, mitigated only by TLS in production. Future
  hardening replaces this with a short-lived signed ticket.
- Joiner-list mutation on WS disconnect during `Starting` / `Active`.
  Server broadcasts `peer_left` but does not modify the persisted
  joiners array (races with `/start` writes).
- Duplicate `Identify` policy. Current behavior: second identify on
  an already-connected player is ignored. Intended: kick the old.

### Tests

42 unit + module tests pass as of this release. The WebSocket handler
itself is verified manually via the new HTML harness — Rust
WebSocket testing is intentionally deferred to avoid adding
`tokio-tungstenite` as a dev-dependency for a single test file.

---

## [0.4.7] — 2026-05-19

Patch release — closes a race in the auto-apply flow that left
`update:previous_version` stale (pointing the rollback button at the
wrong target version after every apply). Pure compose-config change;
no Rust code changes.

### Fixed

- **Autonomous Watchtower polls raced the server's apply path
  (`docker-compose.yml`)** — observed after the v0.4.4 swap from
  `containrrr/watchtower:1.7.1` to `nickfedor/watchtower:1.17.0`. The
  compose previously set `command: --http-api-periodic-polls`, which
  per the [nickfedor docs](https://watchtower.nickfedor.com/v1.17.0/advanced-features/http-api/)
  re-enables periodic polling on top of the HTTP API ("By default,
  enabling the HTTP API prevents periodic updates. The
  `--http-api-periodic-polls` flag re-enables periodic updates while
  the HTTP API is active."). The server's apply path in
  `server/src/update/task.rs::maybe_apply` (and `wait_and_apply` /
  `ApplyNow`) does:
    1. `docker::pull_channel_image` — bollard caches the new image
       locally.
    2. `watchtower::trigger_update` — HTTP POST to Watchtower.
    3. On success, `store_previous_version` — writes
       `update:previous_version = current`.

  With autonomous polling enabled, Watchtower's poll firing between
  steps 1 and 2 saw the new locally-cached image as "newer than
  running" (`WATCHTOWER_NO_PULL=true` skips the registry pull but
  doesn't suppress the local-vs-running comparison) and redeployed
  on its own — killing the outgoing container before step 3 wrote.
  The incoming container booted with `update:previous_version`
  untouched from the previous apply, so the rollback button pointed
  two (or more) versions back instead of one.

  **Fix**: remove `command: --http-api-periodic-polls`. Watchtower
  falls back to its default entrypoint and, with
  `WATCHTOWER_HTTP_API_UPDATE=true` set in env, runs HTTP-API-only —
  no autonomous polls, no race. The server's apply path is now the
  sole driver of container swaps. `WATCHTOWER_NO_PULL=true` is kept
  for defense-in-depth and gets a cross-reference comment.

  **Self-healing**: the next successful apply (v0.4.6 → v0.4.7)
  runs the un-raced path and writes the correct
  `update:previous_version = "0.4.6"`. No manual recovery needed; no
  Redis migration step required.

### Migration

None. Watchtower auto-applies through the existing HTTP API contract
(the server's trigger path is unchanged). The change takes effect on
the next compose recreate, which happens automatically as part of
the Watchtower image being swapped during the v0.4.7 apply.

---

## [0.4.6] — 2026-05-19

Patch release — fixes the "Update Available" green banner that stayed
stuck on the admin dashboard after a successful auto-apply or manual
Apply Now. Rollback button behavior is unchanged (it was correct
already, since it keys off a different Redis state).

### Fixed

- **Stale `update:available_version` not cleared post-apply
  (`update/task.rs`, `admin/dashboard.rs`)** — `run()` writes
  `update:current_version` to the new binary's `env!("CARGO_PKG_VERSION")`
  on startup but did nothing about the `update:available_version` that
  the outgoing binary left behind. The dashboard renders the banner
  whenever that key is non-empty, with no comparison against current
  (`dashboard.rs:78,104`), so the banner stayed green until the next
  auto-check cycle (default 6h) happened to hit the `NoUpdate` branch —
  and only if the just-applied tag was no longer the latest matching
  release for the channel. Now, the startup path reads
  `update:available_version` and clears it (and `update:found_at`) if
  the value is semver-equal to or older than the running version. The
  dashboard render path uses the same predicate as defense-in-depth so
  any state churn (manual `redis-cli SET`, partial cleanup, future code
  that forgets the rule) can't re-stick the banner.

### Added

- **`update::task::is_stale_available_version` helper** — pure semver
  predicate, matches the existing extracted-helper-for-testability
  pattern used by `decide_should_apply`, `should_reset_found_at`,
  `wait_and_apply_should_proceed`, and `classify_recovered_schedule`.
  `pub(crate)` so `admin/dashboard.rs` can reuse it without duplicating
  the comparison logic.
- **Six unit tests** in `server/src/update/task.rs::tests`:
  - `is_stale_available_version_empty_is_not_stale`
  - `is_stale_available_version_dev_prerelease_after_apply_is_stale`
  - `is_stale_available_version_exact_match_is_stale`
  - `is_stale_available_version_newer_is_not_stale`
  - `is_stale_available_version_unparseable_available_is_not_stale`
  - `is_stale_available_version_unparseable_current_is_not_stale`

  Cover the post-apply stuck-banner path on both dev/ea (prerelease) and
  stable channels, defensive behaviour on parse failures (an
  unparseable value left alone), and the genuinely-newer case
  (banner preserved).

### Migration

None. The new binary's first boot runs the cleanup automatically.
Watchtower auto-applies through the existing GitHub Releases contract;
neither was touched. No new Redis keys, no env vars, no dependencies
(`semver` was already a server dep).

---

## [0.4.5] — 2026-05-19

Patch release — switches Redis persistence from a Docker-managed named
volume to a bind mount alongside the compose file. Operator-driven: the
data was previously hidden under `/var/lib/docker/volumes/...`, which
made backup and disaster-recovery workflows harder than they needed to
be. No server code changes.

### Changed

- **`docker-compose.yml` Redis volume** — switched from the named volume
  `redis_data` to bind mount `./redis_data`. The top-level
  `volumes: redis_data:` declaration is removed (bind mounts don't need
  one). Per-environment isolation is preserved by directory layout —
  each env's compose file has its own sibling `./redis_data/` directory —
  rather than by docker-volume name prefix.
- **`.gitignore`** — adds `redis_data/` so operator runtime state isn't
  accidentally committed.
- **Docs** — `architecture.md`, `server-autoupdate.md`, and the env
  isolation section of `briska-blast-ops-manual.md` updated to describe
  the new mount layout. The `docker volume ls | grep redis_data`
  verification example is replaced with `ls -la */redis_data/`.

### Added

- **`docs/migrations/0.4.5-redis-bind-mount.md`** — full one-time
  migration procedure for existing deployments: stop the stack, copy
  AOF data from `/var/lib/docker/volumes/<project>_redis_data/_data/`
  to `./redis_data/`, chown to uid 999 (the `redis:7-alpine` container
  user), pull the new compose, restart, verify admin password still
  works. Old named volume is intentionally left in place as a rollback
  safety net.
- **`docs/migrations/` directory** — establishes
  `docs/migrations/<version>-<slug>.md` as the canonical pattern for
  future one-shot upgrade procedures.

### Migration

**Required for existing deployments.** Without the migration procedure,
Redis starts with an empty `./redis_data/` directory and the admin
password resets to the default `@admin` — admin panel access is
recoverable but all custom Redis state (min versions, update history,
schedules, rollback targets) is lost. See
`docs/migrations/0.4.5-redis-bind-mount.md` for the full procedure.

New deployments need no special steps — `docker compose up` creates the
bind mount target on first start. If Redis logs `Permission denied` on
first boot, `chown -R 999:999 redis_data` and restart.

---

## [0.4.4] — 2026-05-19

Patch release — replaces the archived Watchtower image with a
maintained fork. Compose-only; no server code changes.

### Fixed

- **`docker-compose.yml` Watchtower image pin** —
  `containrrr/watchtower:1.7.1` was archived in December 2025 and only
  speaks Docker Engine API v1.25. Docker Engine 28+ enforces a minimum
  API of v1.44, so the old image crash-loops on modern hosts with:
  `error="client version 1.25 is too old. Minimum supported API version is 1.44"`.
  Swapped to `nickfedor/watchtower:1.17.0` — the actively-maintained
  continuation fork at `github.com/nicholas-fedor/watchtower`. The fork
  autonegotiates the API version, so it transparently supports whatever
  Docker Engine the host is running (verified against Engine 29.1.3,
  API 1.52 / min 1.44). Env vars and CLI flags are unchanged from the
  containrrr image; the swap is a drop-in.

### Migration

None required. `docker compose pull && docker compose up -d` recreates
the watchtower container on the new image, picking up the existing
`WATCHTOWER_TOKEN` and config. Existing crash-looping
`containrrr/watchtower:1.7.1` containers on Docker Engine 28+ will
start working again immediately after the pull.

---

## [0.4.3] — 2026-05-18

Patch release — completes the rollback/auto-apply race fix that v0.4.2 started.
v0.4.2 closed the READ side (auto-apply now re-reads Redis state under
`update_apply_lock`); v0.4.3 closes the WRITE side (rollback's local Docker
retag) plus a related fragility in the Watchtower-failure branch. No new
admin-panel functionality; the rollback button's success path is unchanged,
and the failure-path toast now reflects that auto-update is disabled.

### Fixed — medium

- **Rollback retag could be raced by an in-flight auto-apply
  (`admin/dashboard.rs`)** — `retag_for_rollback` was called BEFORE acquiring
  `update_apply_lock`. A concurrent `maybe_apply` (holding the lock) would
  call `pull_channel_image`, which pulls `:channel` from the registry and
  overwrites the local image ref that rollback just retagged. Depending on
  interleaving the admin would see "Rollback triggered" with the new image
  still live, or auto-apply would silently deploy the rollback image via the
  wrong code path. Lock is now acquired before the retag, so both writers to
  the local `:channel` ref serialise on the same mutex.

### Fixed — smaller

- **Watchtower-failure branch left a fragile state (`admin/dashboard.rs`)** —
  when `watchtower::trigger_update` returned `false`, the local retag
  persisted but the rollback safety lock was NOT set. The persisted retag
  would be picked up correctly by a subsequent *Watchtower* trigger
  (`WATCHTOWER_NO_PULL=true` means Watchtower itself never pulls — see
  containrrr discussion #557), but our own auto-apply paths
  (`maybe_apply` / `wait_and_apply` / `ApplyNow`) call `pull_channel_image`
  from the registry before triggering Watchtower and WOULD overwrite the
  persisted retag. The failure branch now sets `update:rollback_locked=true`
  and `update:auto_enabled=false`, so subsequent auto-apply bails out under
  `should_apply_after_lock`. `update:previous_version`, `available_version`,
  and `found_at` are intentionally NOT deleted in the failure branch — the
  rollback did not complete, so those values remain valid for a retry. The
  failure toast now informs the admin that auto-update is disabled.
- **Misleading comment on the Watchtower-failure branch
  (`admin/dashboard.rs`)** — previously claimed "Watchtower's normal pull on
  next attempt would handle the same operation"; this is wrong because
  `WATCHTOWER_NO_PULL=true` was set in v0.4.1. Rewritten to describe the
  actual reasoning behind the new safety lock.
- **`update_apply_lock` doc comment broadened (`state.rs`)** — the lock now
  serialises local Docker `:channel` ref mutations as well as Redis state
  writes around the Watchtower trigger. Comment updated, and a note added
  about the single-writer assumption (the lock is sufficient only as long as
  this Rust process is the only writer to the daemon's `:channel` ref; the
  Docker Engine API exposes no native tag/ref lock primitive — see moby
  PR #37781).

### Migration

None. No new Redis keys, no new env vars, no new dependencies, no new admin
endpoints. The rollback button's success-path behaviour is byte-identical;
the failure-path now sets the same safety lock the success path does and
the toast message communicates it. v0.4.2 can discover and apply v0.4.3
through the existing GitHub Releases / Watchtower contract — neither was
touched.

---

## [0.4.2] — 2026-05-18

Patch release — post-review correctness pass over the update system. Addresses
findings from the in-branch review of v0.4.1 before first production deploy.
No new admin-panel functionality; existing buttons (check / apply / schedule /
cancel / settings / rollback / auto-toggle) behave identically on the happy
path. The fixes are all on the failure / concurrent / tampered-input edges.

`RELEASE_CHANNEL` remains baked at compile time via `build.rs` — unchanged on
purpose. Defense-in-depth against runtime env tampering.

### Fixed — critical

- **Rollback-defeating race in auto-apply (`update/task.rs`)** — `maybe_apply`
  previously read `update:scheduled_at`, `update:available_version`, etc.
  *before* acquiring `update_apply_lock`. A rollback that completed inside
  that window (acquiring the lock first, setting `auto_enabled=false` /
  `rollback_locked=true`, retagging the local image) could be silently undone
  by the auto-apply path resuming, re-pulling the registry's `:channel` image,
  and re-triggering Watchtower. Lock is now acquired first; authoritative
  state is re-read inside the lock via the new `decide_should_apply` predicate
  (auto_enabled / rollback_locked / scheduled_at / available_version). Same
  fix applied to `wait_and_apply` and `UpdateCommand::ApplyNow`.
- **Stable channel accepted non-`-ea`/`-dev` prereleases (`update/github.rs`)** —
  the previous `channel_matches` used substring matching for `-ea` and `-dev`.
  Tags like `v1.2.3-beta.1` or `v1.2.3-rc.1` slipped through onto stable
  whenever GitHub's `prerelease` flag was misconfigured as false. Filter now
  parses the tag via `semver::Version` and inspects `Version::pre.is_empty()`
  for stable; ea/dev still require the `prerelease` flag *and* the matching
  pre-release identifier prefix. Unparseable tags now match nothing.

### Fixed — high

- **Rollback handler trusted the form field (`admin/dashboard.rs`)** —
  `rollback_update` formatted `form.version` directly into a Docker image tag
  with no validation. A tampered POST (admin session required, but the hidden
  field is cosmetic) could deploy any tag that exists in GHCR. Now validates
  the value as semver and cross-checks it against `update:previous_version`
  read from Redis; mismatch redirects with an error. Redis is the source of
  truth, not the form.
- **`WATCHTOWER_TOKEN` had a hardcoded fallback (`config.rs`)** — compose
  enforces `${WATCHTOWER_TOKEN:?...}` fail-closed, but the binary itself fell
  back to the literal `"briska-watchtower-token"` when the env var was unset.
  Non-compose runs (dev shell, manual deploy, custom orchestrator) silently
  booted with a publicly-known token. Fallback removed; missing env var now
  panics on startup with an actionable message.
- **`apply_update_now` had no precondition (`admin/dashboard.rs`)** — sent
  `UpdateCommand::ApplyNow` regardless of whether an update was actually
  available. The UI hides the button, but a crafted POST would still trigger
  a no-op Watchtower call. Now reads `update:available_version` and refuses
  if empty; also refuses if `update:rollback_locked == "true"`.

### Fixed — smaller

- **`wait_and_apply` cleared the schedule before the apply succeeded
  (`update/task.rs`)** — `clear_schedule_conn` now runs only after
  `watchtower::trigger_update` returns `true`. A failed apply no longer
  silently drops the schedule.
- **`update:previous_version` was written before Watchtower accepted the
  trigger (`update/task.rs`, `admin/dashboard.rs`)** — could leave a stale
  rollback target equal to the still-running version. Moved to the success
  branch in every apply path. The rollback handler already wrote on success;
  the other three paths now match.
- **`update:last_checked` was written before the GitHub call (`admin/dashboard.rs`,
  `update/task.rs`)** — a failed check still advanced the dashboard timestamp,
  misleading operators. Now written only on `Ok(...)` outcomes; `Err(...)`
  leaves the previous value intact.
- **Hand-rolled `urlencoding` in `admin/dashboard.rs`** — only escaped space
  and colon; any error message containing `&`, `=`, `?`, `#`, `+`, or `%`
  produced malformed redirect URLs. Replaced with the `urlencoding` crate.
- **`update:manual_override` was dead state (`admin/dashboard.rs`,
  `admin/templates.rs`)** — read on every dashboard render, threaded into
  `DashboardData` as `_update_manual_override`, never written anywhere, never
  consumed. Removed.
- **GitHub pagination boundary warning (`update/github.rs`)** — when the
  releases response returns exactly 100 entries (the `per_page` cap), log a
  `tracing::warn!` flagging that newer releases may be off-page. No behaviour
  change; just operator visibility before "auto-update mysteriously stopped"
  hits the field.

### Added

- **Unit tests for the new predicates** — `decide_should_apply` exercised
  across the four bail-out conditions; `channel_matches` regression-tested
  against `-beta`, `-rc`, `-alpha`, and unparseable tags. 14 update-module
  tests pass; previous 6 still pass.

### Dependencies

- Added `urlencoding = "2"`.

### Migration

None. All `update:*` Redis key names and meanings are preserved. v0.4.1 can
discover and apply v0.4.2 through the existing GitHub Releases / Watchtower
contract — neither was touched. The pre-merge checklist in
`docs/server/changing-the-update-system.md` was walked end-to-end.

---

## [0.4.1] — 2026-05-18

Patch release — pre-deployment hardening pass over the update system. No
behavioural surface change for clients (launcher/game); all changes are
internal correctness, observability, and security improvements.

### Fixed / Changed

- **GitHub Releases check (`update/github.rs`)** — three changes to the polling logic:
  - Conditional `If-None-Match: <etag>` on subsequent requests; ETag stored in Redis under `update:github_etag`. A 304 response now flows through a new `CheckOutcome::NotModified` arm that preserves all cached state.
  - Optional `Authorization: Bearer <GITHUB_TOKEN>` header when the `GITHUB_TOKEN` env var is set; absence does not change behaviour. Raises the anonymous 60 req/hr/IP limit to 5000 req/hr authenticated.
  - URL now requests `?per_page=100`; matching releases are collected, parsed via `semver::Version`, sorted descending, and the largest is returned if `> current`. Removes the implicit "GitHub returns newest-first" dependency.
- **Update task error visibility (`update/task.rs`)** — every Redis call that previously hid errors behind `.unwrap_or(())` or `.unwrap_or_default()` now logs via `tracing::warn!` through an `.inspect_err(...)` chain. Control flow and fallback behaviour are unchanged; transient Redis blips now surface in operator logs instead of disappearing silently.
- **Single-flight apply lock** — `AppState::update_apply_lock: Arc<tokio::sync::Mutex<()>>` added. Acquired by every code path that calls `watchtower::trigger_update`:
  - `task.rs::run` — `UpdateCommand::ApplyNow` arm
  - `task.rs::maybe_apply` — timer-driven auto-apply
  - `task.rs::wait_and_apply` — scheduled apply
  - `admin/dashboard.rs::rollback_update` — admin rollback path

  Watchtower itself is idempotent; the lock prevents concurrent writes of `update:previous_version` / `update:available_version` / `update:found_at` from interleaving.
- **30-day sanity cap on recovered schedules** — `task.rs::classify_recovered_schedule` is consulted on startup. A `scheduled_at` more than 30 days in the future is treated as corruption: a warning is logged and the Redis keys are cleared. Prevents a malformed value from spawning a `wait_and_apply` future that sleeps for years.
- **Auto-apply pre-pull** — both auto-apply paths (`ApplyNow`, `maybe_apply`, `wait_and_apply`) now call `update::docker::pull_channel_image(channel)` before triggering Watchtower. Required because Watchtower is now configured with `WATCHTOWER_NO_PULL=true` (see Compose changes below).
- **`docker-compose.yml`** — security and correctness updates:
  - Watchtower image pinned from `:latest` (implicit) to `:1.7.1`.
  - Watchtower service moved from `ports:` (host-published, even loopback-only) to `expose:` (internal Docker network only). The Watchtower HTTP API endpoint is no longer reachable from the host shell.
  - `WATCHTOWER_NO_PULL=true` added to Watchtower env. Required so the rollback flow's local retag isn't silently overwritten by a registry pull on Watchtower's next trigger.
  - `WATCHTOWER_TOKEN` default value removed from compose. Variable now uses the `${WATCHTOWER_TOKEN:?...}` interpolation guard; `docker compose up` fails fast with an error message if `.env` is missing the token, instead of silently running with the literal `briska-watchtower-token`.
- **`.gitignore`** — was empty; now ignores `target/`, `.env*` (except `.env.example` is committed), and common IDE/OS junk. Belt-and-suspenders against committing future secrets.
- **`.env.example`** — `WATCHTOWER_TOKEN` is now documented as required (with a placeholder, not a default). New `GITHUB_TOKEN` line documents the optional auth token.

### Code documentation added (no behaviour change)

- `update/docker.rs` — block comment on `retag_for_rollback` documenting the synchronous admin-handler call path (no `UpdateCommand::Rollback` variant exists) and the `WATCHTOWER_NO_PULL=true` requirement.
- `update/docker.rs` — block comment on the `IMAGE_REPO` const explaining why it stays hardcoded (defense-in-depth against env-var redirection).
- `state.rs` — comment on the new `update_apply_lock` field describing the invariant.

### Tests added

`server/src/update/github.rs::tests`:
- `stable_channel_rejects_prereleases` — channel matching for `stable` rejects `-ea`, `-dev`, and any `prerelease=true` tag.
- `ea_channel_accepts_only_ea` — only accepts `-ea` tags that are also marked `prerelease`.
- `dev_channel_accepts_only_dev` — only accepts `-dev` tags that are also marked `prerelease`.
- `unknown_channel_matches_nothing` — defensive: unknown channel names return no matches.
- `semver_prerelease_ordering` — pins `1.2.3-ea.10 > 1.2.3-ea.2 > 1.2.3-ea.1` and `1.2.3 > 1.2.3-ea.10`, guarding the descending-sort path.

`server/src/update/task.rs::tests`:
- `found_at_only_resets_on_new_tag` — repeated polls returning the same tag must NOT reset `update:found_at`. Tests the extracted `should_reset_found_at` helper which now drives the live `do_check` decision.
- `wait_and_apply_proceeds_only_on_exact_match` — a spawned `wait_and_apply` future no-ops if the stored `update:scheduled_at` is missing, different, or even one second off (cancel-then-reschedule). Tests the extracted `wait_and_apply_should_proceed` helper.
- `recovered_schedule_classification` — 30-day-future cap, exact-now boundary, year-2099 corruption are each classified correctly.

### Verified-as-already-fine (no change needed)

- **Finding 1 (rollback wiring)**: `update::docker::retag_for_rollback` was suspected unwired. Reading `admin/dashboard.rs`, it is called synchronously from the `rollback_update` handler (`POST /admin/update/rollback`). Per the prompt's own instruction, code was left intact and a call-path comment was added in `docker.rs`.
- **Finding 2 (`update:scheduled_version` inconsistency)**: the key IS set — but in the admin handler (`admin/dashboard.rs::schedule_update`), not in `task.rs`. The admin handler sets it immediately before sending `UpdateCommand::Schedule`. `task.rs::clear_schedule_conn` correctly deletes it on cancel / stale recovery. The system invariant holds; no change.
- **Finding 14 (`WATCHTOWER_LABEL_ENABLE=true`)**: already correctly set on the Watchtower service. The server container correctly carries the `com.centurylinklabs.watchtower.enable=true` label. No change.

### Declined

- **Finding 9 (`IMAGE_REPO` to config)**: the prompt's literal request was to move `IMAGE_REPO` to a runtime env var with the current GHCR URL as default. After review with the project owner, this was declined for security reasons: making the update target runtime-configurable would let a compromised environment (`.env` tampering, container env injection) redirect the update / rollback path at a malicious registry. The const stays hardcoded in `docker.rs`; forks needing a different registry must rebuild. A comment on the const records this rationale.

### Deferred (Security Notes)

For tracking — items 11, 12, 13, 14, 15 from the audit prompt and their status:

| # | Item | Status | Disposition |
|---|---|---|---|
| 11 | Server has direct Docker socket access via `bollard`; compromise == host root. | **Deferred** to follow-up branch `harden/docker-socket-proxy`. | Introduce `tecnativa/docker-socket-proxy` and restrict the server's permissions to only `images:read`, `images:create`, and `containers:write` — the minimum that rollback retag + pre-pull need. |
| 12 | Watchtower HTTP API exposure. | **Fixed in this release**. | `ports:` → `expose:`. |
| 13 | Bearer token literal default in compose. | **Fixed in this release**. | Default removed, `${WATCHTOWER_TOKEN:?...}` interpolation guard added. `.gitignore` now covers `.env`. |
| 14 | `WATCHTOWER_LABEL_ENABLE=true` + server label. | **Already correct.** | No change. |
| 15 | Watchtower image pinned. | **Fixed in this release**. | Pinned to `containrrr/watchtower:1.7.1`. |

### Risk notes for the human reviewer

- The auto-apply path now requires the server container to have Docker socket access (already mounted for rollback). This is the same blast radius as before — `bollard` was already in use — but now an additional code path uses it. Strengthens the case for item 11.
- `WATCHTOWER_NO_PULL=true` makes Watchtower's update behaviour entirely server-driven: Watchtower will never act on its own anymore. If `pull_channel_image` ever fails silently *and* the existing image is unchanged, the apply path becomes a no-op. The bollard error is logged at `warn` level — operators should alert on this.
- `WATCHTOWER_TOKEN` is now a deploy-blocking required env var. Existing `.env` files without it will refuse to `docker compose up`. Document this in the deploy runbook.

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
  - **Server Bind Address** — save a new bind address to Redis; applied on next container restart
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
