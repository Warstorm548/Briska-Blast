# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

BriskaBlast is a cross-platform multiplayer online game targeting Windows and Linux. Packages: `client`, `launcher`, `server`, `shared`, `tools`. See [`docs/architecture.md`](docs/architecture.md) for full structure.

## Technology Stack

- **Client**: Rust + Bevy
- **Launcher**: Rust + Iced
- **Server**: Go
- **Platforms**: Windows + Ubuntu/Linux

## Key Design Constraints

- `shared/` must be platform-agnostic — no browser APIs or OS-specific built-ins. See [`docs/protocol.md`](docs/protocol.md).
- Dev tools ship as separate files on the `dev` branch only — stable and experimental installs never receive them. See [`docs/devtools.md`](docs/devtools.md).
