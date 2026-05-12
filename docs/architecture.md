# Architecture

BriskaBlast is split into four top-level packages plus build tooling.

## Client (`client/src/`)
Rust + Bevy project using ECS — game logic lives in systems and components, not OOP hierarchies.
- `core/` — app lifecycle, bootstrapping
- `game/` — game loop, entities, state
- `rendering/` — draw calls, scene graph, shaders (GLSL in `assets/shaders/`)
- `networking/` — connection to relay server, message handling
- `input/` — keyboard/mouse/gamepad handling
- `audio/` — sound playback, asset loading
- `ui/` — menus, HUD, overlays
- `assets/` — static assets: sprites, fonts, shaders, audio, config

## Server (`server/src/`)
Go server handling real-time relay and session management.
- `relay/` — real-time message relay between players
- `session/` — per-game session state management
- `matchmaking/` — lobby and player matching logic
- `api/` — HTTP endpoints (auth, stats, etc.)
- `config/` — environment/runtime configuration

## Shared (`shared/`)
Platform-agnostic code imported by both client and server — no browser APIs, no OS-specific built-ins.
- `protocol/` — message types and serialization for client↔server communication
- `types/` — shared domain types
- `utils/` — pure utility functions

## Launcher (`launcher/`)
Rust + Iced standalone app that runs before the game. See [`devtools.md`](devtools.md) for the dev branch channel.
- `src/ui/` — launcher window, screens, layout components
- `src/auth/` — login, account creation, token/session storage
- `src/updater/` — core update engine
  - `branches/` — manifest fetching and branch switching
  - `downloader/` — file fetching and integrity verification
  - `patcher/` — applying diffs and binary swapping
- `src/news/` — patch notes feed, server status, announcements
- `src/settings/` — launcher preferences and game launch options
- `src/devtools/` — dev-branch-only overlay (hidden by default)
- `src/networking/` — shared HTTP client and CDN helpers
- `src/config/` — runtime config, env vars, launch flags
- `assets/` — launcher-specific backgrounds, icons, fonts
- `tests/` — launcher integration tests

## Tools (`tools/`)
- `build/` — build scripts and bundler configuration
- `dev/` — local dev helpers (hot reload, dev server, etc.)
