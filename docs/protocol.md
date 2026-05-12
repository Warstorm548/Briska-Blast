# Protocol

## Source of Truth

`shared/protocol/` is the single source of truth for all client↔server message shapes. Any change to a message type must be made there first — never define message structures independently in `client/` or `server/`.

## Constraints

- `shared/` must remain fully platform-agnostic: no browser APIs, no Node.js/OS built-ins.
- Serialization format and message types are defined in `shared/protocol/` and imported by both sides.

## Message Flow

```
Client (Rust/Bevy)  <-->  shared/protocol/  <-->  Server (Go relay)
```

All relay messages pass through the Go relay server in `server/src/relay/`. Session state is managed in `server/src/session/`.
