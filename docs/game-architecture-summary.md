# Game Architecture Design Summary
### Cross-Platform Multiplayer Game with Launcher, Client, and Server Relay

---

## Overview

This document summarizes the complete architecture designed across our conversation for a real-time multiplayer ball game with peer-to-peer networking, a game launcher, and a matchmaking relay server.

---

## Tech Stack Decisions

| Component | Technology | Reason |
|---|---|---|
| Launcher | Rust | Fast file I/O, single binary, great for system-level tasks |
| Game Client | Godot 4 + C# | Cross-platform via export templates, native WebRTC support |
| Server | Rust + Axum | Maximum performance, control, and scalability |
| Async Runtime | Tokio | Handles thousands of concurrent connections |
| Session Storage | Redis | In-memory hashmap, TTL auto-expiry, microsecond lookups |
| Containerization | Docker + Docker Compose | Portable, easy redeployment across servers |
| CI/CD | GitHub Actions | Auto-build and deploy on push to main |
| Process Management | Systemd / Docker restart policies | Keeps containers alive |
| Admin Interface | Built-in `/admin` panel | Password-protected web UI for runtime config |

---

## Cross-Platform Strategy

### Rust (Launcher + Server)
- Cross-compile using target triples
- `rustup target add x86_64-pc-windows-gnu`
- `cargo build --target x86_64-pc-windows-gnu`
- OS-specific code uses `#[cfg(target_os = "windows")]` attributes

### Go (Alternative considered)
- `GOOS=linux GOARCH=amd64 go build` — environment variable based
- Build constraints via `//go:build linux` file annotations

### Godot 4 + C# (Game Client)
- Code stays the same across platforms
- Export templates handle platform differences
- `OperatingSystem.IsWindows()` / `OperatingSystem.IsLinux()` for platform checks
- Export presets defined per target OS in the Godot editor

---

## Component Breakdown

### 1. Launcher (Rust)

**Responsibilities:**
- Download and install game files
- Manage game branches (like git branches for game versions)
- Read and write player settings
- Store Player ID and secret token locally (separate from game saves)
- Spawn the game process

**Key Note:** Built last in development order — needs a working game to launch.

---

### 2. Matchmaking Server (Rust + Axum + Redis + Docker)

**Two Core Endpoints:**
```
POST /host → generate session code, store IP + port in Redis
POST /join → receive session code, look up and return host IP + port
```

**Session Object Structure:**
```
Session {
    code: "ABC123",
    game_mode: "multiplayer",
    settings: { ...host configured settings... },
    status: "waiting" | "active" | "ended",
    host_id: PlayerID,
    players: [
        {
            id: PlayerID,
            ip: "X.X.X.X",
            port: 54231,
            score: 0,
            status: "connected" | "disconnected" | "reconnecting",
            reconnect_timer: Option<30s>,
        }
    ],
    ttl: 30 minutes
}
```

**Session Rules:**
- Sessions auto-expire after 30 minutes via Redis TTL
- No new players allowed to join after session starts (may vary by game mode)
- Host disconnects → 30 second grace period to reconnect
- Non-host disconnects → 30 second grace period, barrier appears on their screen edge
- If host does not return → promote next player in chronological join order
- If only 1 player remains → session ends immediately
- If all players disconnect → session expires immediately

**Host Promotion Queue (Chronological Join Order):**
```
[Player A (host), Player B, Player C, Player D]

Player A disconnects → Player B becomes host
Player B disconnects → Player C becomes host
Player C disconnects → 1 player remains → session ends
```

**Score Validation:**
- Ball packets sent to both receiving player AND server
- Server independently calculates expected trajectory
- Score claims checked against server's tracked trajectory
- Invalid score claims rejected
- Anti-cheat thresholds deferred to post-foundation phase

**Docker Compose Structure:**
```
Docker Compose
    ├── Axum Server (Rust)
    │     ├── POST /host
    │     └── POST /join
    └── Redis
          └── Sessions auto-expire after 30 minutes
```

**GitHub Actions Deployment Flow:**
```
Push to main branch
    → GitHub Actions builds new Docker image
    → Pushes image to Docker Hub or GitHub Container Registry
    → Server pulls new image
    → Container restarts with updated code
```

---

### 3. Game Client (Godot 4 + C#)

**Main Menu Options:**
- Host a game
- Join a game (via session code)
- Solo player vs computer
- Settings
- Cosmetics (future)

**Networking:** WebRTC for P2P connections (handles NAT traversal automatically)

---

## Networking Architecture

### The NAT Traversal Problem

Home routers act like apartment building front desks — outgoing traffic passes freely, but unsolicited incoming connections are blocked. Two NATed peers can't connect "cold." The solution is to use **WebRTC**, which handles NAT traversal automatically via the ICE protocol (STUN candidates for the common case, TURN relay for symmetric NATs).

In v0.5.0, the server is no longer a "matchmaking broker that hands out IP:ports." The server is a **signaling channel**: it relays small text messages (SDP offers, answers, ICE candidates) between peers over a WebSocket. WebRTC discovers and uses the actual peer endpoints client-side — the server never sees or stores them.

### WebRTC Signaling Flow (v0.5.0)

```text
Player A (Host)          Server (signaling)          Player B (Joiner)
     |                         |                          |
     |-- POST /host ---------->|                          |
     |   {gamemode, count}     |                          |
     |<- {session_code} -------|                          |
     |                         |<-- POST /join -----------|
     |                         |    {code}                |
     |                         |--> {gamemode, count,     |
     |                         |     joiner roster} ----->|
     |-- WS identify --------->|                          |
     |                         |<-- WS identify ----------|
     |<-- peer_joined ---------|                          |
     |                         |                          |
     |-- POST /start --------->|                          |
     |<-- start_signaling -----|--> start_signaling ----->|
     |                         |                          |
     |== SDP/ICE relayed via the WebSocket both ways =====|
     |                         |                          |
     |<======= direct WebRTC peer connection ============>|
     |======= server is no longer in the data path ======|
```

**Key Insight:** The server brokers the introduction (via signaling) but never touches game data after the WebRTC peer connection is established.

### STUN and TURN
- **STUN** lets each peer discover its own public IP:port through a public STUN server (the project uses `stun.l.google.com:19302`). The browser/Godot WebRTC stack does this transparently — application code only sees ICE candidates ready to exchange.
- **TURN** relays peer traffic for the ~5–10% of consumer routers that are "symmetric NATs" and can't be hole-punched. v0.5.0 ships **without** TURN — affected players currently cannot join sessions. See [`roadmap.md`](roadmap.md).

### P2P Topology (4 Players)

**Interest Management:** Players only receive data relevant to their screen.

```
Player A ←→ Player B    (ball moving between them)
Player A ←→ Player C    (ball moving between them)
Player B ←→ Player D    (ball moving between them)
No unnecessary connections!
```

When the ball leaves Player A's screen heading toward Player B — Player A sends data **directly** to Player B only. Players C and D receive no data unless the ball enters their screen.

---

## Ball Synchronization

### Dead Reckoning
When a packet is delayed, clients don't freeze or wait. They continue simulating the ball forward using the last known state:

```
Last known position: (x, y)
Last known velocity: (dx, dy)
Timestamp of that data
→ Keep simulating forward until next correction packet arrives
```

### Packet Contents
```
Ball packet:
    position: (x, y)
    velocity: (dx, dy)
    trajectory: (calculated arc)
    timestamp: (when sent)
    from_player: A
    to_player: B
```

### Reconciliation (Smooth Correction)
When a correction packet arrives and predicted position differs from actual:

- **Small difference** → smoothly interpolate (lerp) over 3-5 frames
- **Huge difference** → snap (indicates something went very wrong)

Smooth interpolation keeps the ball visually trackable for players reacting with paddles.

### Score Validation Flow
```
Player A hits ball toward Player B
    → Ball packet sent to Player B AND server
    → Server calculates expected trajectory

Player B claims score against Player A
    → Server checks: did trajectory support ball passing collision barrier?
        Yes → score valid, update session
        No  → reject score claim
```

---

## Player Identity System

### First Launch Flow
```
Game launches for first time
    → Contacts server
    → Server generates unique Player ID + Secret Token
    → Saved locally (separate file from game saves)
    → Launcher also references this ID
```

### Identity File (stored on player's computer)
```
{
    player_id: "0000001",
    secret_token: "k9mX2$nP8qL..."  ← long random string, never share
}
```

### Two-Part Identity (Hotel Analogy)
- **Player ID** `0000001` → room number, short, readable, sequential
- **Secret Token** `k9mX2$nP8qL...` → key card, long, random, impossible to guess

Both together prove identity. Neither alone is sufficient.

### Server Storage
```
Server stores:
    player_id: "0000001"
    secret_token: [hashed version]  ← even server breach can't expose real tokens
```

### ID Scaling
```
Players 1–9,999,999     → 0000001   (7 digits)
Player 10,000,000       → 00000001  (8 digits, adds a digit)
Player 100,000,000      → 000000001 (9 digits, adds another digit)
```

### Lost Identity File (Reinstall / New Computer)
- No ID file found → same flow as first launch
- New identity generated
- Previous data is lost (accepted tradeoff — no cloud saves planned)
- Future: Steam/Epic store integration would replace this with their auth system

### Reconnection Validation Flow
```
Player A disconnects
    → 30 second grace period starts
    → Player A reconnects
    → Sends player_id + secret_token
    → Server validates both match stored record
    → Confirms genuine Player A
    → Restores host status if applicable
```

---

## Build Order (Why It Matters)

### Wrong Instinct
Build what you can see first: Launcher → Game → Server

### Why It Fails
- Launcher needs game files to install — but game isn't built yet
- Game needs P2P connections to test — but server isn't built yet
- Can't test anything end to end

### Correct Order (Dependency Chain)
```
1. Server first    → everything depends on this
2. Game second     → P2P testable immediately once server exists
3. Launcher last   → game already works, launcher just wraps it
```

**Rule:** Always ask "what does everything else depend on?" — that goes first.

---

## Future Systems (Post-Foundation)

These are intentionally deferred to keep scope manageable:

- Anti-cheat tolerance thresholds
- Cheat flagging and banning
- Score anomaly detection
- Steam/Epic Games store authentication
- Cloud save integration
- Cosmetics system
- Additional game modes

---

## Key Concepts Learned

| Concept | Definition |
|---|---|
| NAT Traversal | Technique for establishing connections between computers behind home routers |
| NAT Hole-Punching | Both clients simultaneously reach outward so routers see return traffic |
| STUN | Protocol for discovering your own external IP and port as seen by the internet |
| Signaling Server | Server that brokers P2P introductions without relaying game data |
| External Mapped Port | The port your router assigns externally, may differ from local listen port |
| Dead Reckoning | Continuing to simulate ball movement using last known trajectory during packet delay |
| Reconciliation | Smoothly correcting predicted position when a correction packet arrives |
| Interest Management | Only sending data to players who need it (relevant screen only) |
| Host Promotion Queue | Ordered list of players for host handoff on disconnect |
| TTL (Time To Live) | Redis feature that auto-deletes session data after a set time |
| Docker Compose | Tool for defining and running multiple containers together |
| Target Triple | Rust's way of specifying OS + architecture for cross-compilation |
| UUID / Token Auth | Two-part identity: readable ID + secret token for secure validation |
