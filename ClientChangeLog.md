# Client Changelog

All notable changes to the Briska Blast game client are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
