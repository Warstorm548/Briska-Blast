# Protocol

## Source of Truth

`shared/protocol/` is the single source of truth for all client↔server message shapes. Any change to a message type must be made there first — never define message structures independently in `client/` or `server/`.

## Constraints

- `shared/` is a Rust library crate shared between `server/` and `launcher/`. It must remain fully platform-agnostic: no browser APIs, no OS-specific built-ins.
- The Godot 4 + C# client uses equivalent types defined in `client/src/` — it cannot directly import the Rust shared crate.
- Serialization format: JSON over HTTP for REST endpoints; JSON-text frames for WebSocket signaling.

## Message Flow

```
Godot 4 + C# Client  <-->  Rust + Axum Server  <-->  Redis (session state)
                                    ^
                             shared/protocol/
                        (Rust types: server + launcher)
```

Once WebRTC peer connections are established, the server is **not** in the game-traffic data path. Players talk directly to each other; the server is a signaling channel only.

## REST Endpoints

| Endpoint | Method | Auth | Purpose |
|---|---|---|---|
| `/register` | POST | none | First-contact: issue `player_id` + `secret_token` |
| `/host` | POST | token | Host requests a session with a `gamemode` + `player_count`; receives a session code |
| `/join` | POST | token | Joiner submits the code; receives the gamemode, capacity, and current roster |
| `/session/{code}` | GET | none | Poll session status, capacity, and joiner roster |
| `/session/{code}` | DELETE | token (host) | Explicit session teardown — frees the code immediately |
| `/session/{code}/start` | POST | token (host) | Transition lobby Waiting → Starting and trigger signaling |
| `/session/{code}/host` | POST | token (host) | Voluntarily hand the host role to a listed joiner (Waiting only); broadcasts `HostChanged` |

All authenticated POSTs carry `player_id` + `secret_token` in the JSON body. Token validation: SHA-256 of the supplied token compared against `player:<id>:token_hash` in Redis.

`/host`, `/join`, `/session/{code}/start`, and `/session/{code}/host` are version-gated — see [Version Enforcement](#version-enforcement) below. `/ws/session/{code}` is not — clients are already gated by the REST step they used to learn the code.

## WebSocket Endpoint

| Endpoint | Method | Auth | Purpose |
|---|---|---|---|
| `/ws/session/{code}` | GET (Upgrade) | identify frame | WebRTC signaling channel for one player in one session |

The client must send `{"type":"identify","player_id":"…","secret_token":"…"}` as the first text frame within 5 seconds of the upgrade. The server validates the token, confirms the player is a member of `session:{code}` in Redis, registers them in the in-process SignalHub, and replies with `Identified`. Any other initial frame closes the connection with code `4400`.

Close codes (4xxx, app-defined):

| Code | Reason | When |
|---|---|---|
| `4400` | `identify_required` | First frame is not `Identify`, or arrives after the 5s deadline |
| `4401` | `unauthorized` | Token doesn't match the stored hash |
| `4403` | `not_in_session` | Authenticated, but not the host or a listed joiner |
| `4404` | `session_not_found` | The session code has no Redis entry (deleted, TTL'd, or never existed) |
| `4500` | `internal` | Redis fault, decoding failure, etc. |

After `Identified`, the client and server exchange the messages documented in `server/src/signaling/protocol.rs` (`ClientMsg` for incoming, `ServerMsg` for outgoing). The server attests `from` on every relayed message based on the authenticated WS connection — clients cannot forge a `from` field.

Lobby lifecycle frames (server → client): `PeerJoined`, `PeerLeft { reason }` (`"disconnect"` for a dropped socket, `"leave"` for a deliberate `Leave`), `HostChanged { player_id }` (host role moved — currently only via a voluntary `/session/{code}/host` transfer), `SessionEnded { reason }`, and `Kicked { reason }`.

## End-to-end signaling flow

```text
Host                    Server                Joiner(s)
 |-- POST /host -------->|                       |
 |     {gamemode,        |                       |
 |      player_count}    |                       |
 |<- {session_code} -----|                       |
 |                       |<-- POST /join --------|
 |                       |    {code}             |
 |                       |-> {gamemode,          |
 |                       |    player_count,      |
 |                       |    current_count,     |
 |                       |    joiners} --------->|
 |                       |                       |
 |-- WS /ws/session/X -->|                       |
 |-- identify ---------->|                       |
 |<-- identified --------|                       |
 |                       |<-- WS /ws/session/X --|
 |                       |<-- identify ----------|
 |                       |--> identified ------->|
 |<-- peer_joined -------|                       |
 |                       |                       |
 |-- POST /start ------->|                       |
 |   (broadcast)         |                       |
 |<-- start_signaling ---|                       |
 |                       |--> start_signaling -->|
 |                       |                       |
 |-- offer (to=joiner) ->|--> offer (from=host) >|
 |<- answer (from=j) ----|<- answer (to=host) ---|
 |<>=== ICE candidates relayed both ways =======<>|
 |                                               |
 |<======= direct WebRTC peer connection =======>|
 |======= server is no longer in the path =======|
```

Multi-peer sessions follow the same pattern but every pair exchanges its own offer/answer/ICE — `n` players form an `n*(n-1)/2` mesh.

## Version Enforcement

The server reads `X-Launcher-Version` and `X-Game-Version` on the version-gated REST endpoints. If either is below the corresponding minimum stored in Redis (`min_launcher_version`, `min_game_version`), the server returns `426 Upgrade Required` with a body that names which component is stale. The minimum versions are runtime-configurable via the admin panel — no redeploy needed.

WebSocket upgrade does NOT carry version headers. Browsers cannot easily attach custom headers to a WS upgrade, and clients are already version-gated when they reach the WS via `/host` or `/join`.
