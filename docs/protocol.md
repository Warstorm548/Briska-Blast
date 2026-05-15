# Protocol

## Source of Truth

`shared/protocol/` is the single source of truth for all client↔server message shapes. Any change to a message type must be made there first — never define message structures independently in `client/` or `server/`.

## Constraints

- `shared/` is a Rust library crate shared between `server/` and `launcher/`. It must remain fully platform-agnostic: no browser APIs, no OS-specific built-ins.
- The Godot 4 + C# client uses equivalent types defined in `client/scripts/` — it cannot directly import the Rust shared crate.
- Serialization format: JSON over HTTP for all signaling endpoints.

## Message Flow

```
Godot 4 + C# Client  <-->  Rust + Axum Server  <-->  Redis (session state)
                                    ^
                             shared/protocol/
                        (Rust types: server + launcher)
```

After the signaling handshake completes, the server is not involved in game traffic. Both clients communicate directly via UDP hole-punch.

## Signaling Endpoints

| Endpoint | Method | Purpose |
|---|---|---|
| `/register` | POST | First-contact: issue player ID + secret token |
| `/host` | POST | Host registers external IP:port, receives session code |
| `/join` | POST | Joiner submits external IP:port + code, receives host IP:port |
| `/session/{code}` | GET | Host polls to discover when joiner has arrived |
| `/session/{code}` | DELETE | Explicit session teardown (frees code immediately) |

## Hole-Punch Flow

```
Host                    Server                   Joiner
  |-- POST /host -------->|                         |
  |<-- {session_code} ----|                         |
  |                        |<-- POST /join ----------|
  |                        |    {code, joiner_ip}   |
  |                        |--> {host_ip} ----------|
  |-- GET /session/{code}->|                         |
  |<-- {joiner_ip:port} ---|                         |
  |<======= simultaneous UDP to each other =========>|
  |<============= direct P2P connection =============>|
```

## Version Enforcement

The server reads `X-Launcher-Version` on `/host` and `/join` requests. If the version is below `min_launcher_version` (stored in Redis), the server returns `426 Upgrade Required`. The minimum version is runtime-configurable — no redeploy needed.
