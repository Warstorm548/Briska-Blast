# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

BriskaBlast is a multiplayer online game. The codebase is split into three top-level packages: `client`, `server`, and `shared`. Build tooling lives in `tools/`.

## Architecture

### Client (`client/src/`)
Browser/app-side code organized by concern:
- `core/` — app lifecycle, bootstrapping
- `game/` — game loop, entities, state
- `rendering/` — draw calls, scene graph, shaders (GLSL in `assets/shaders/`)
- `networking/` — connection to relay server, message handling
- `input/` — keyboard/mouse/gamepad handling
- `audio/` — sound playback, asset loading
- `ui/` — menus, HUD, overlays
- `assets/` — static assets: sprites, fonts, shaders, audio, config

### Server (`server/src/`)
Node.js game server:
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

### Tools (`tools/`)
- `build/` — build scripts and bundler configuration
- `dev/` — local dev helpers (hot reload, dev server, etc.)

## Key Design Constraint

Anything in `shared/` must remain platform-agnostic — no browser APIs, no Node.js built-ins. The relay protocol lives in `shared/protocol/` and is the single source of truth for client↔server message shapes.
