# Observability & logging

Permanent, structured logging across the game client and the server. Built to make
the WebRTC/handoff path visible (peers that fail to connect are otherwise silent —
see [`extended-mode.md`](extended-mode.md) §"Ball travel"), and to give every
future issue a durable trail. This is the reference for **where logs go, how they're
shaped, and when a client log file begins and ends**.

## Client (Godot + C#)

### The `Log` API

`client/src/core/Log.cs` is an autoload registered in `project.godot` **after
`SingleInstance`** (so it can honour the duplicate-instance check). It exposes
static, fail-safe methods — nothing here may throw into the game:

```csharp
Log.Info("net.webrtc", $"peer={pid} connection {a}->{b}");
Log.Warn("game.handoff", $"DROPPED ball={id} peer={pid} — channel not open (ball lost)");
```

- **Levels:** `Trace < Debug < Info < Warn < Error`. `MinLevel` defaults to **Debug
  on the `dev` channel, Info on ea/stable**, so release builds don't spew per-frame
  detail. It's a public setter — a future in-game toggle can raise/lower it live.
- **Categories** (free-form tags, current vocabulary): `boot`, `net.webrtc`,
  `net.signaling`, `game`, `game.handoff`, `session`, `identity`, `single-instance`,
  `fatal`.
- **Line format:** `[HH:mm:ss.fff] [LEVEL] [category] message`.
- Every line is **mirrored to the Godot console** (so terminal runs still show it and
  Warn/Error light up the editor) **and** written to the per-run file below.

### The log file — one file per game process

| Aspect | Behaviour |
|---|---|
| **Location** | `<data_dir>/log<channel>/` — `logdev` / `logea` / `logstable`, under the launcher-known per-user data dir (`Paths.LogDir()`). |
| **Filename** | `briskablast_<channel>_<YYYY-MM-DD_HH-mm-ss>.log` (channel in the name too, so a file sent on its own is self-identifying). |
| **Begins** | When the winning instance boots (autoload `_EnterTree`). A **rejected duplicate** instance writes no file. |
| **Ends** | When the process terminates — clean exit flushes/closes; a crash/hang ends at the last flushed line (`AutoFlush` keeps the tail on disk). |
| **Not split by** | Menus, match transitions, `GameOver`, disconnects. One sitting = one file, with in-file `session <CODE>` markers. A new file appears only on **relaunch**. |
| **Retention** | Newest **20** runs per channel folder; older pruned on open. |

`<data_dir>` is resolved by `client/src/core/Paths.cs` (`DataDir()`), which mirrors
the launcher's Rust `directories`/`paths.rs` layout exactly — macOS/Windows append
`/data`, Linux is lowercase with no suffix. That's why the launcher's **Logs button**
can open the same folder (`launcher/src/paths.rs::logs_dir`).

> **Why a custom C# sink, not Godot's built-in `file_logging`?** Godot's `user://`
> can't be made byte-identical cross-platform to the launcher's data dir, and its
> logger path is frozen at boot — so it can't land in the launcher-discoverable
> per-channel folder. The C# sink writes to the path the game already resolves.
> Trade-off: a hard *native* crash isn't captured by a C# sink; mitigated by
> AutoFlush. Add a Godot built-in backstop later only if a native crash is suspected.

### What's instrumented

- **`WebRtcMeshTransport`** — ICE connection-state transitions, gathering state, ICE
  **candidate types** (host/srflx/relay = the NAT story), data-channel open/close,
  offerer/answerer role. A stalled/failed peer link is diagnosable from the log alone.
- **`NetGameController`** — each ball handoff in/out, plus a **Warn when a handoff is
  sent to a not-yet-open channel** (`IPeerTransport.Send` now returns a success bool),
  which is the previously-silent drop behind "no balls crossing".
- **`SignalingClient` / `SessionContext` / `SingleInstance` / `GameScene`** — connect/
  reconnect, identity, single-instance rendezvous, and session lifecycle.

## Server (Rust + tracing)

- **Per-connection span** on the signaling WebSocket handler
  (`signaling/ws/mod.rs::handle_socket`) carries `session` and `player`, so every line
  while a socket is live — including the relay in `frame.rs` — is correlatable.
- **Relay trace** (`debug`): offer/answer/ICE relays; ICE with its candidate type.
- **`PeerConnectionFailed`** logs at **WARN** with structured `peer`/`reason` fields —
  this is the direct server-side signature of a failed WebRTC pair.
- **`LOG_FORMAT`** env (via `Config`): `pretty` (default) or `json`. `json` feeds log
  shippers and the future admin **Logs tab** ([`../planning/roadmap.md`](../planning/roadmap.md)).
  Level still comes from `RUST_LOG` (default `server=info,tower_http=info`).

## Diagnosing "balls don't cross portals"

1. **Client log** (`log<channel>/…`): look for `net.webrtc` — does the data channel
   reach `OPEN`, or does `connection` stall in `Checking` / hit `Failed`? Candidate
   types tell the NAT story (srflx present but no connectable pair ⇒ traversal
   failing). A `game.handoff DROPPED …` Warn means the ball left but the channel
   wasn't open.
2. **Server log**: a `peer connection failed` WARN with `reason=ice_failed` is the
   direct confirmation. Filter by the `session` field to isolate one match.
3. If ICE never connects across the internet but works on a LAN → symmetric NAT; the
   fix is the deferred **TURN relay** (roadmap).
