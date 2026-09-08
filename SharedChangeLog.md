# Shared Changelog

All notable changes to the Briska Blast `shared` crate (the platform-agnostic
protocol/types library) are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> This changelog was introduced at **0.4.0**. The earlier entries (0.1.0–0.3.0)
> are reconstructed from git history — version numbers come from `shared/Cargo.toml`
> at each bump, and the changes are derived from the commits that touched
> `shared/src/` in that range. They may be less granular than entries written at
> release time. The C# client mirrors these wire types by hand
> (`client/src/net/Dto.cs`); bumps here usually imply a matching client change.

---

## [0.7.0] — 2026-09-07

Adds the loot-table rules type behind the game's first hotbar item.

### Added

- `LootSettings` (`types/loot_settings.rs`) — host-configured loot rules, serialized
  flat as `{"drop_interval_secs":20,"barrier_enabled":true,"barrier_weight":50,
  "barrier_duration_secs":30}`. Bounds exported as constants and hand-mirrored in
  `client/src/net/Dto.cs`: drop interval 5–60s (default 20), weight 1–100
  (default 50), Full Barrier duration 5–120s (default 30).
- `LootSettings::subscribed_total` / `resolved_rates` / `nothing_rate` — the drop
  odds. Weights are **buckets**: the total sums the *distinct* weights of enabled
  items, and items tied on a weight split that bucket evenly. So a lone item at 50
  drops on half of all rolls, and two items both at 50 drop on 25% of rolls each
  rather than subscribing 100% between them. The remainder up to 100 is the chance
  nothing drops.
- `LootSettings::validate` + `LootSettingsError` — per-field range checks plus a hard
  100% cap on the subscribed total. Unlike the other settings types this error names
  the offending **field**, because the loot table has four independently-bounded
  values and "invalid loot settings" alone would not tell a host which slider to move.
- `loot_settings` field (`#[serde(default)]`) on `HostRequest`, `JoinResponse` and
  `SessionPollResponse`, so an older client that omits it still hosts with defaults.

The weighting maths lives here rather than client-side because this is the only place
in the stack with a test runner that CI-adjacent work actually executes
(`cargo test -p shared`); `LootTable.cs` mirrors it.

---

## [0.6.0] — 2026-07-12

Re-tunes the **Set Score** win condition bounds: default **100**, range
**50–200** (was default 11, range 10–50).

### Changed

- **`WinCondition` Set-Score constants** (`types/win_condition.rs`):
  `SET_SCORE_MIN 10 → 50`, `SET_SCORE_MAX 50 → 200`, `DEFAULT_TARGET 11 → 100`.
  Still a `u8` (200 ≤ 255) and the same wire shape
  `{"kind":"set_score","target":N}` — only the validated range and default move,
  so the client mirror (`Dto.cs`) and the server's `/host` `validate()` both
  follow from this single source. Unit tests updated to the new bounds/default.

---

## [0.5.0] — 2026-06-29

Adds **`SpawnSettings`** — the host-configured random-spawn rules (BallSpliter
spawn cadence + chain-splitting) for the new ball-splitter mechanic.

### Added

- **`SpawnSettings`** (`types/spawn_settings.rs`) — flat wire shape
  `{"splitter_interval_secs":N,"chain_split":bool}` with shared range constants
  (`SPLITTER_INTERVAL_MIN_SECS`/`_MAX`, default 15s; `chain_split` default on) and a
  `validate()` for the server's trust boundary — same single-source convention as
  `WinCondition`.
- **`spawn_settings` field** on `HostRequest`, `JoinResponse` and
  `SessionPollResponse` (`#[serde(default)]`), carried alongside `win_condition` so an
  older client that omits it still hosts with the defaults.

---

## [0.4.0] — 2026-06-22

Adds the first **win condition** type.

### Added

- **`WinCondition`** (`types/win_condition.rs`) — internally tagged on `kind`, wire
  shape `{"kind":"set_score","target":N}`. The single `SetScore { target }` variant
  ends a match when a player reaches `target`. Ships the shared range constants
  (`SET_SCORE_MIN = 10`, `SET_SCORE_MAX = 50`, `DEFAULT_TARGET = 11`), a `validate()`
  returning `(min, max, requested)` on failure, `target()`, and a `Default`
  (`SetScore { target: 11 }`). Single source of truth for the client's input cap and
  the server's authoritative `/host` check, like `MAX_USERNAME_LEN`.
- `win_condition: WinCondition` on `HostRequest`, `JoinResponse`, and
  `SessionPollResponse` (mirrors `gamemode`: set after host setup, echoed to joiners).

---

## [0.3.0] — 2026-06-20

Tightens the username cap and gives a rejected rename something to revert to.

### Added

- **`MAX_USERNAME_LEN: usize = 20`** in `protocol::messages` — the single shared
  username cap consumed by both the launcher's input limit and the server's
  `/register` / `/me/username` trust-boundary check, so the UX limit and the
  authoritative check can't drift.
- **`UpdateUsernameResponse { username }`** — always carries the username the server
  now has on file (the new name on accept, the unchanged stored name on a `422`
  reject), so a tampered/over-length rename snaps the client back to the last
  stable value.

---

## [0.2.0] — 2026-05-26

Builds out the multiplayer session protocol (typed game mode, N-player sessions,
and the full session lifecycle). Accumulates the `shared/src` changes from the
staging commits between 0.1.0 and this version bump.

### Added

- **Typed `GameMode` enum** (`types/gamemode.rs`, snake_case wire form) replacing
  ad-hoc strings, threaded onto `HostRequest` / `JoinResponse` / `SessionPollResponse`.
- **N-player session fields**: `player_count` + `current_player_count` on the
  host/join/poll types, and the `JoinedPeer` roster entry.
- **Session-lifecycle request types**: `RegisterRequest`, `StartSessionRequest`,
  `TransferHostRequest`, `UpdateUsernameRequest`.
- **`dev_flag: bool`** on `RegisterResponse` (server-side per-user gate for the dev
  channel UI).

### Changed

- `SessionStatus` now spans the full lifecycle — `Waiting`, `Starting`, `Active`,
  `Ended` (the `Starting` transition state was added here).

---

## [0.1.0] — 2026-05-15

Initial `shared` crate, landed with the server's first cut.

### Added

- **Protocol types** (`protocol/messages.rs`): `RegisterResponse`, `HostRequest`,
  `HostResponse`, `JoinRequest`, `JoinResponse`, `SessionPollResponse`,
  `CloseSessionRequest`.
- **`PlayerId`** newtype with `from_counter` (zero-padded id formatting) and the
  `SessionStatus` enum.
