# Game Changelog

All notable changes to the Briska Blast game are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> Renamed from `ClientChangeLog.md` on 2026-05-23 to match the
> `game-v*` release-tag namespace and avoid confusion with launcher /
> server changelogs. Content prior to the rename is preserved below.

---

## [0.2.0] — 2026-05-23

Launcher-handoff protocol. The game can now receive its display username
from the launcher via a one-shot temp JSON file, so the launcher's "Play"
button can deliver per-channel identity into the game without exposing
it on the command line. Standalone launches (game run directly, no
launcher) continue to work with the existing placeholder username.

### Added

- **`LaunchArgs.FromLauncher`** (`src/core/LaunchArgs.cs`) — parses
  `--launcher-handoff <path>` from `OS.GetCmdlineArgs()`, reads the
  JSON, **deletes the file on read**, and caches the result for the
  rest of the process lifetime. Tolerant of missing arg, missing file,
  malformed JSON — returns `null` so callers can fall back gracefully.
- **`SessionContext.LocalUsername`** (`src/core/SessionContext.cs`) —
  populated from the handoff in `_Ready()`. `StartHostSession` now seeds
  `PlayerNames` from this value instead of a hardcoded literal. Falls
  back to `"Player Username 1"` if no handoff was provided so dev runs
  outside the launcher behave unchanged.

### Handoff schema

The launcher writes (and the game consumes + deletes) a JSON object:

```jsonc
{ "username": "BlastQueen99" }
```

The schema is intentionally object-shaped — future fields like
`player_id`, `secret_token`, and `server_url` (roadmap) can be added
without breaking the contract.

### Notes

- This release is the first under the `game-v*` GitHub-Release tag
  namespace. Stage 2 of the launcher install pipeline wires the CI to
  publish artifacts when `game-v0.2.0-dev.1` is pushed.
- No multiplayer endpoints yet; this is groundwork only for the
  launcher's Install / Update / Play flow.

---

## [0.1.0] — 2026-05-21

First versioned client build. UI scaffold only — no networking yet, no
playable game scene yet. Establishes the project shape, menu navigation,
shared visual theme, and an in-memory session lobby suitable for visual
testing of host/non-host roles.

### Added

- **Godot 4.6.3 .NET project scaffold** under `client/` (`project.godot`,
  `BriskaBlast.csproj`, `.gitignore`). Targets `net8.0`; nullable enabled;
  design viewport 2560×1440 matching `BackgroundDefault.png`; window starts
  at 1280×720 for laptop-friendly first launch; canvas-items stretch mode
  with `expand` aspect so UI scales cleanly across monitor sizes.

- **Main menu** (`src/ui/menus/MainMenu.tscn`): Solo Play (disabled
  placeholder), Host Game, Join Game, Customization (disabled placeholder),
  Settings, Exit Game.

- **Host Setup menu** (`src/ui/menus/HostSetupMenu.tscn`): Basic / Advanced
  tabs, Game Mode dropdown (Extended only for now), Max Players spinbox
  (range driven by selected mode, 2–4 for Extended including host),
  Create Session → Session Lobby.

- **Join menu** (`src/ui/menus/JoinMenu.tscn`): six-character session-code
  input with uppercase normalization, Enter-to-submit, length validation,
  Return-to-main-menu.

- **Settings menu** (`src/ui/menus/SettingsMenu.tscn`) with four tabs:
  - **Display** — window mode, resolution (filtered to monitor capability),
    UI scale slider, VSync mode, FPS cap, renderer (restart required).
  - **Graphics** — MSAA, texture filtering, shadow quality, post-FX.
  - **Audio** — Master/Music/SFX/UI sliders, output device.
  - **Game** — Show FPS, Show Ping, HUD scale, text size, paddle-controls
    rebind placeholder.

  Apply button persists to `user://settings.cfg` and applies live; Return
  discards staged changes.

- **Session Lobby** (`src/ui/menus/SessionLobby.tscn`): three-column
  layout. Left — session code (placeholder `ABC123`), current settings,
  Return-to-Setup. Center — Start Session. Right — Connected Players list
  with four fixed slots, promote-to-host buttons, and a chat scaffold
  (RichTextLabel log + LineEdit input; not wired). Cancel/Leave action at
  bottom-left whose text adapts to host vs non-host role.

- **`SettingsManager` autoload** (`src/core/SettingsManager.cs`):
  ConfigFile-backed; applies display settings at startup.

- **`SessionContext` autoload** (`src/core/SessionContext.cs`): in-memory
  session state (code, mode, max players, player list, host index,
  local-is-host flag) that survives scene transitions. Placeholder data
  only — no server interaction.

- **Shared menu theme** (`src/ui/theme/MenuTheme.tres`): blue Button base
  with cyan neon-glow on hover (via StyleBoxFlat `shadow_color` +
  `shadow_size`), darker pressed, greyed disabled. Matching LineEdit
  styling for SpinBox and code input — same neon focus halo.

- **Debug helpers in Session Lobby** (`#if DEBUG` only):
  F1 toggle host view, F2 add placeholder player, F3 remove last player.
  Lets us exercise host-vs-non-host UI without networking.

- **Version tracking** — `config/version="0.1.0"` in `project.godot`,
  exposed via `BriskaBlast.Core.GameVersion.Current`.

### Notes

- No game scene yet — Start Session and Solo Play are intentional no-ops.
- No server contact yet — Host/Join flows seed `SessionContext` with
  placeholder data (`"ABC123"`, `"Player Username N"`) so the lobby UI
  can be exercised end-to-end before networking lands.
- All buttons in the host-setup → lobby flow operate purely on
  in-memory state; safe to click without a running server.
