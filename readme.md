# BriskaBlast

BriskaBlast is a fast-paced cross-platform multiplayer game where players compete online across Windows and Linux. Built around quick matches and real-time gameplay, the goal is to blast your way past opponents and be the last one standing. The game is currently in early development — features, mechanics, and content are subject to change as the project grows.

## Platform Support

BriskaBlast is playable on **Windows** and **Linux** with full cross-platform multiplayer — players on either platform can compete against each other in the same match.

## How to Get the Game

The game is launched through the **BriskaBlast Launcher**, which handles installation, updates, and login. The launcher automatically keeps your game up to date and lets you choose which release channel to play on:

- **Stable** — the main public release, recommended for most players
- **Experimental** — early access to upcoming changes, may have rough edges

## Documentation

The following docs are intended for contributors and developers working on the project.

**Architecture & design**

| Document | Description |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Overview of how the codebase is structured — packages, folders, and what each part is responsible for |
| [`docs/game-architecture-summary.md`](docs/game-architecture-summary.md) | Higher-level design summary covering the cross-platform multiplayer game, launcher, client, and server relay |
| [`docs/protocol.md`](docs/protocol.md) | How the client and server communicate — message format rules and where message types are defined |

**Development**

| Document | Description |
|---|---|
| [`docs/setup.md`](docs/setup.md) | How to set up a local development environment and get the project running |
| [`docs/testing.md`](docs/testing.md) | How to run tests across the client, server, and launcher |
| [`docs/devtools.md`](docs/devtools.md) | The hidden dev branch channel and how developer tools are shipped separately from public builds |
| [`docs/workflows.md`](docs/workflows.md) | GitHub Actions CI and release workflows — what they do, how to enable them, and the tools used |

**Operations**

| Document | Description |
|---|---|
| [`docs/briska-blast-ops-manual.md`](docs/briska-blast-ops-manual.md) | Practical reference for running the server stack on a dedicated host — port allocation, nginx, environments, troubleshooting |
| [`docs/server-autoupdate.md`](docs/server-autoupdate.md) | Server self-update system: release channels, Watchtower, admin panel update controls, Redis state keys |
| [`docs/changing-the-update-system.md`](docs/changing-the-update-system.md) | **Read before modifying any update-path code or `docker-compose.yml`.** Pre-merge checklist, risk categories, and safe-evolution patterns for the self-update system |

**Launcher & versioning**

| Document | Description |
|---|---|
| [`docs/launcher-update-and-version-validation.md`](docs/launcher-update-and-version-validation.md) | How the launcher updates itself and how server-side version gates enforce protocol compatibility |

**Forward-looking / planned**

| Document | Description |
|---|---|
| [`docs/pocket-id-integration.md`](docs/pocket-id-integration.md) | Design for replacing the admin password with passkey-only OIDC auth via Pocket ID (not yet implemented) |

**Project tracking**

| Document | Description |
|---|---|
| [`docs/bugs.md`](docs/bugs.md) | Known issues and how to report bugs |
| [`docs/session-notes.md`](docs/session-notes.md) | Personal notes — things to pick up in the next dev session |
