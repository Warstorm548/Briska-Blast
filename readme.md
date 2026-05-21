# BriskaBlast

BriskaBlast is a real-time multiplayer game for 2 to 4 players. Matches run peer-to-peer between the connected players; a small matchmaking server handles host/join introductions but steps out of the gameplay path once the peer connection is established.

The project is in early development — gameplay mechanics, content, and the user experience are still taking shape.

## Platform Support

BriskaBlast targets **Windows** and **Linux** with full cross-platform multiplayer — players on either platform can compete against each other in the same match.

## Current Status

Pre-alpha. Components are built in dependency order — server first, then game client, then launcher.

| Component | Status |
|---|---|
| Matchmaking server (Rust + Axum + Redis) | **v0.5.1** — deployed. Player registration, session host/join with 2–4 player capacity, WebSocket signaling for WebRTC peer setup, admin panel, self-update system. |
| Game client (Godot 4 + C#) | **v0.1.0** — menu UI scaffold only. No playable game scene yet; no networking yet. |
| Launcher (Rust + Iced) | Not started. Players will eventually install and update the game through it. |

Component changes are tracked in their respective changelogs: [`ServerChangeLog.md`](ServerChangeLog.md), [`ClientChangeLog.md`](ClientChangeLog.md).

## How to Get the Game

> The launcher is not yet built. Until it ships, the game can only be run by contributors from source — see the development docs below.

When released, BriskaBlast will be launched through the **BriskaBlast Launcher**, which will handle installation, updates, and login. The launcher will let you choose which release channel to play on:

- **Stable** — the main public release, recommended for most players
- **Early Access (EA)** — opt-in to upcoming changes, may have rough edges

## Documentation

The following docs are intended for contributors and developers working on the project.

**Architecture & design**

| Document | Description |
|---|---|
| [`docs/architecture/architecture.md`](docs/architecture/architecture.md) | Overview of how the codebase is structured — packages, folders, and what each part is responsible for |
| [`docs/architecture/game-architecture-summary.md`](docs/architecture/game-architecture-summary.md) | Higher-level design summary covering the cross-platform multiplayer game, launcher, client, and server relay |
| [`docs/architecture/protocol.md`](docs/architecture/protocol.md) | How the client and server communicate — message format rules and where message types are defined |

**Development**

| Document | Description |
|---|---|
| [`docs/dev/setup.md`](docs/dev/setup.md) | How to set up a local development environment and get the project running |
| [`docs/dev/testing.md`](docs/dev/testing.md) | How to run tests across the client, server, and launcher |
| [`docs/dev/devtools.md`](docs/dev/devtools.md) | The hidden dev branch channel and how developer tools are shipped separately from public builds |
| [`docs/dev/workflows.md`](docs/dev/workflows.md) | GitHub Actions CI and release workflows — what they do, how to enable them, and the tools used |

**Operations**

| Document | Description |
|---|---|
| [`docs/server/briska-blast-ops-manual.md`](docs/server/briska-blast-ops-manual.md) | Practical reference for running the server stack on a dedicated host — port allocation, nginx, environments, troubleshooting |
| [`docs/server/server-autoupdate.md`](docs/server/server-autoupdate.md) | Server self-update system: release channels, Watchtower, admin panel update controls, Redis state keys |
| [`docs/server/changing-the-update-system.md`](docs/server/changing-the-update-system.md) | **Read before modifying any update-path code or `docker-compose.yml`.** Pre-merge checklist, risk categories, and safe-evolution patterns for the self-update system |

**Launcher & versioning**

| Document | Description |
|---|---|
| [`docs/launcher/launcher-update-and-version-validation.md`](docs/launcher/launcher-update-and-version-validation.md) | How the launcher updates itself and how server-side version gates enforce protocol compatibility |

**Forward-looking / planned**

| Document | Description |
|---|---|
| [`docs/server/pocket-id-integration.md`](docs/server/pocket-id-integration.md) | Design for replacing the admin password with passkey-only OIDC auth via Pocket ID (not yet implemented) |

**Project tracking**

| Document | Description |
|---|---|
| [`docs/planning/bugs.md`](docs/planning/bugs.md) | Known issues and how to report bugs |
| [`docs/planning/session-notes.md`](docs/planning/session-notes.md) | Personal notes — things to pick up in the next dev session |
