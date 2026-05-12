# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

BriskaBlast is a cross-platform multiplayer online game targeting Windows and Linux. The codebase is split into three top-level packages: `client`, `server`, and `shared`. Build tooling lives in `tools/`.

## Technology Stack

- **Client**: Rust + Bevy — game engine and all client-side game logic
- **Launcher**: Rust + Iced — standalone launcher application (updater, auth, news)
- **Server**: Go — real-time relay and game session management
- **Shared**: Protocol definitions and types used by both sides
- **Platforms**: Windows + Ubuntu/Linux (cross-platform play)

## Architecture

### Client (`client/src/`)
Rust + Bevy project organized by concern:
- `core/` — app lifecycle, bootstrapping
- `game/` — game loop, entities, state
- `rendering/` — draw calls, scene graph, shaders (GLSL in `assets/shaders/`)
- `networking/` — connection to relay server, message handling
- `input/` — keyboard/mouse/gamepad handling
- `audio/` — sound playback, asset loading
- `ui/` — menus, HUD, overlays
- `assets/` — static assets: sprites, fonts, shaders, audio, config

### Server (`server/src/`)
Go game server:
- `relay/` — real-time message relay between players
- `session/` — per-game session state management
- `matchmaking/` — lobby and player matching logic
- `api/` — HTTP endpoints (auth, stats, etc.)
- `config/` — environment/runtime configuration

### Shared (`shared/`)
Code imported by both client and server:
- `protocol/` — message types and serialization for client↔server communication
- `types/` — shared domain types
- `utils/` — pure utility functions with no platform dependencies

### Launcher (`launcher/`)
Rust + Iced standalone app that runs before the game:
- `src/ui/` — launcher window, screens, and layout components
- `src/auth/` — login, account creation, token/session storage
- `src/updater/` — core update engine
  - `branches/` — manifest fetching and branch switching (stable / experimental / dev)
  - `downloader/` — file fetching and integrity verification
  - `patcher/` — applying diffs and binary swapping
- `src/news/` — patch notes feed, server status, announcements
- `src/settings/` — launcher preferences and game launch options
- `src/devtools/` — dev-branch-only overlay; hidden unless the account has the dev flag
- `src/networking/` — shared HTTP client and CDN helpers
- `src/config/` — runtime config, env vars, launch flags
- `assets/` — launcher-specific backgrounds, icons, fonts
- `tests/` — launcher integration tests

**Branch channels**: `stable` (public release), `experimental` (opt-in testing), `dev` (hidden; grants access to in-game dev tools via `devtools/`).

### Tools (`tools/`)
- `build/` — build scripts and bundler configuration
- `dev/` — local dev helpers (hot reload, dev server, etc.)

## Key Design Constraints

- Anything in `shared/` must remain platform-agnostic. The relay protocol in `shared/protocol/` is the single source of truth for client↔server message shapes.
- The client is a Bevy ECS project — game logic is expressed as systems and components, not OOP hierarchies.
