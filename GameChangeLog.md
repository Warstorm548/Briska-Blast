# Game Changelog

All notable changes to the Briska Blast game are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

> Renamed from `ClientChangeLog.md` on 2026-05-23 to match the
> `game-v*` release-tag namespace and avoid confusion with launcher /
> server changelogs. Content prior to the rename is preserved below.

---

## [0.2.4] — 2026-05-24

Fourth (and hopefully last) iteration on the headless .NET export.
v0.2.3 still shipped a 64 KB `.pck`. Re-read the **verbose** Godot
output instead of trusting the verify step's surface message, and
the real error was screaming on the first export attempt — buried
mid-`savepack` so it scrolled past the first three investigations:

```
ERROR: Export .NET Project: This project contains C# files but
no solution file was found at the following path:
  /home/runner/work/Briska-Blast/Briska-Blast/client/BriskaBlast.sln
A solution file is required for projects with C# files.
```

Repeated once per `.cs` file. `GodotTools.Export.ExportPlugin._ExportFile`
bails on each, so `BuildManager.PublishProjectBlocking` never fires —
no managed DLLs in the `.pck`. Godot still exits 0 (the known silent
failure: godotengine/godot#86591, #98225), so CI marches forward and
only the post-export `pck_size` canary catches it.

### Why v0.2.1, v0.2.2, and v0.2.3 all missed this

The repo carries `client/BriskaBlast.csproj` but no
`client/BriskaBlast.sln`. On a desktop, the Godot editor auto-creates
the `.sln` the first time the project opens; in headless CI nothing
ever does. The three previous fixes all tweaked the `dotnet
publish` / `dotnet build` / `--editor --quit` ordering — none of which
produces a `.sln`. So each fix was rearranging deck chairs while
ExportPlugin kept bailing at the same earlier point.

The `.sln` is just a plain-text list of which `.csproj` projects belong
to the solution — `dotnet new sln` + `dotnet sln add` produces the same
file the editor would.

### Fixed

- **`.github/workflows/release-client.yml`**: new `Generate solution
  file` step, run right before `dotnet restore`. Runs
  `dotnet new sln --name BriskaBlast` then
  `dotnet sln BriskaBlast.sln add BriskaBlast.csproj` inside `client/`.
  Gives the export plugin the `.sln` it requires; not committed to git
  (matches the editor-side lifecycle on desktop checkouts).

### Unchanged from v0.2.3

- The v0.2.3 `dotnet build` (Debug) step is left in. It becomes moot
  once the `.sln` exists (the export plugin's own publish handles
  the assembly state), but we changed exactly **one** variable this
  iteration so the next failure — if any — has a clean attribution.
  Cleanup to a follow-up PR.
- Still no manual `dotnet publish`, no `--editor --quit` warm-up.
- `dotnet/embed_build_outputs=true` on both presets.
- `--verbose` on `--export-release`.
- Verify-pck-size canary — kept; it caught the last three failures.

### Not touched

- **`.github/workflows/ci-client.yml`** would hit the same issue if
  ever triggered (it's `workflow_dispatch`-only today, so it isn't
  blocking anything). Adding the same step there is a follow-up.

---

## [0.2.3] — 2026-05-24

Third iteration on the headless .NET export. v0.2.2's verify step
caught another empty-of-C# .pck. The verbose log was decisive:

```
.NET: GodotPlugins initialized
.NET: Failed to load project assembly      ← happens HERE, before any publish
[scan filesystem]
ERROR: Failed to create an autoload, script 'SettingsManager.cs' is not compiling.
```

The assembly load failure is in Godot's RUNTIME initialisation,
**before** the export plugin's `BuildManager.PublishProjectBlocking`
would fire. Autoload registration then cascades because the C# scripts
can't compile against a missing assembly. By the time the export
plugin runs, the project is in a broken state and the publish is
skipped — no C# in the .pck.

### Why v0.2.2 didn't reach the publish step

Godot's editor-mode runtime reads the project assembly from
`.godot/mono/temp/bin/<Configuration>/<AssemblyName>.dll`. The default
configuration when not specified is `Debug`. We weren't building Debug
at all in v0.2.2 (the maintainer-confirmed repo assumed clean bootstrap
auto-builds), so `bin/Debug/BriskaBlast.dll` didn't exist, and the
assembly load failed at runtime init.

### Fixed

- **`.github/workflows/release-client.yml`**: re-added a single
  `dotnet build` step (default Debug, no `-c` flag) right after
  `dotnet restore` and before `Pre-import resources`. Populates
  `bin/Debug/BriskaBlast.dll` so Godot's runtime can load the
  assembly during initialisation. Autoload registration then
  succeeds, and the export plugin can do its own publish for the
  export configuration.

### Unchanged from v0.2.2

- Still no manual `dotnet publish` (Godot's export plugin handles it
  once autoloads work).
- Still no `--editor --quit` warm-up (placebo).
- `dotnet/embed_build_outputs=true` in `export_presets.cfg`.
- `--verbose` on `--export-release`.
- Verify-pck-size canary — kept as the safety net.

### If this still fails

Option C from research: build artifacts locally with the Godot editor
GUI (which works), CI reduced to packaging + uploading. Headless export
on 4.6.3 in this environment may simply be broken in a way that needs
manual workaround.

---

## [0.2.2] — 2026-05-24

Iteration on v0.2.1's headless .NET export fix. The v0.2.1 workflow
added an editor warm-up + explicit `dotnet publish` + `dotnet build
-c Release` thinking these would populate the paths Godot's export
plugin needs. They didn't — the verify step (correctly) caught the
broken artifact and refused to publish. v0.2.2 reads Godot 4.6's
ExportPlugin source directly and strips the placebos.

### Why v0.2.1 didn't work

Reading `godotengine/godot@4.6` source:
- `modules/mono/editor/GodotTools/.../Export/ExportPlugin.cs` shows
  ExportPlugin **internally** runs `dotnet publish` to
  `<ProjectBaseOutputPath>/godot-publish-dotnet/<Configuration>-<RID>/`
  and reads the assembly back from there.
- `modules/mono/editor/.../Sdk.props` sets the SDK output to
  `.godot/mono/temp/bin/<Configuration>/` — `bin/Debug/` for the
  default editor and the Godot-internal `godot-publish-dotnet/`
  subdir for exports.
- Our v0.2.1 manual `dotnet publish -c ExportRelease -r linux-x64`
  wrote to `.godot/mono/temp/bin/ExportRelease/linux-x64/{,publish/}` —
  a directory Godot never reads. Same for our `dotnet build -c
  Release` (`bin/Release/` is also dead code from Godot's perspective).
- `godot --headless --editor --quit` warm-up tried to load the project
  assembly from `bin/Debug/`, found nothing (we only built Release),
  failed silently, did nothing useful.

A maintainer-confirmed working pipeline
([Stalker2106/godot-ci-test](https://github.com/Stalker2106/godot-ci-test/blob/master/.github/workflows/main.yaml))
runs only `godot --headless --export-release` on a clean checkout —
no manual `dotnet build`, no `dotnet publish`, no `--editor --quit`.

### Fixed

- **`.github/workflows/release-client.yml`**: removed the three
  placebo steps from v0.2.1 (`dotnet build --configuration Release`,
  `Editor warm-up`, `dotnet publish (Linux/Windows ExportRelease)`).
  Kept the single useful addition: `dotnet restore` to warm the NuGet
  cache, and the verify-pck-size canary. The workflow now matches the
  minimal-working pattern in the maintainer-confirmed reference repo.

### Unchanged from v0.2.1

- `dotnet/embed_build_outputs=true` on both presets (still correct).
- `--verbose` on `--export-release` (still useful for diagnostics).
- Verify steps after each export — the canary that caught v0.2.1's
  failure, kept in place to catch any future regression too.

### No game behaviour changes

- Same as v0.2.1 — this is exclusively a release-pipeline iteration.

---

## [0.2.1] — 2026-05-24

Fix-only release for the **headless `.NET` export pipeline**. v0.2.0
shipped a published Windows zip containing only `BriskaBlast.exe` and
`BriskaBlast.pck` — no managed assemblies, no sibling
`data_BriskaBlast_*` folder. The Godot runtime started, found nothing
to load, and the window closed immediately on the user's machine.

### Diagnosis

The published zip was confirmed corrupted-by-design via:

```
$ python3 -c "import zipfile; print(zipfile.ZipFile('windows.zip').namelist())"
['BriskaBlast.pck', 'BriskaBlast.exe']
```

Online research traced this to two known Godot 4.x issues:

- `godot --headless --export-release` does NOT auto-compile C# the
  way the editor path does ([Godot Forum: "no data folder created on
  export"](https://forum.godotengine.org/t/no-data-folder-created-on-export/110235),
  [Godot issue #87434](https://github.com/godotengine/godot/issues/87434)).
- Silent failures of `BuildManager.PublishProjectBlocking()` inside the
  export plugin still report exit code 0 ([Godot issue #98225 —
  "headless mono export missing dotnet assemblies"](https://github.com/godotengine/godot/issues/98225)).

So our CI happily published a runnable-but-empty artifact each game tag.

### Fixed

- **`client/export_presets.cfg`**: `dotnet/embed_build_outputs=false` →
  `true` on **both** Linux and Windows presets. Bundles the managed
  assemblies INTO the `.pck` — single-file distribution, no sibling
  `data_*` folder to lose.
- **`.github/workflows/release-client.yml`** gains three new steps per
  platform that close the silent-failure hole at every layer:
  1. **Editor warm-up** — `godot --headless --verbose --editor --quit`
     populates `.godot/mono/temp/bin/ExportRelease/<RID>/` with the C#
     build state Godot's `ExportPlugin` reads from. Per the JetBrains
     TeamCity guide on Godot .NET CI builds.
  2. **Explicit `dotnet publish -c ExportRelease -r <RID>
     --self-contained false`** before each export — does the publish
     ourselves so a silent failure of Godot's internal invocation
     cannot ship a broken artifact.
  3. **`--verbose` on the `--export-release`** so any remaining
     publish issue surfaces in the log.
- **Verify steps** after each export assert `pck_size >= 5 MiB` (the
  scaffold + menus + autoloads + embedded .NET DLLs comfortably exceed
  this; an empty-of-C# `.pck` does not). Failure halts CI with a
  `::error::` annotation so we'll never again silently publish an
  artifact lacking managed assemblies.

### Verification

- `python3 -c "import yaml; yaml.safe_load(open('release-client.yml'))"`
  parses clean.
- `cd client && dotnet build --configuration Release` clean
  (csproj untouched).
- Live verification deferred to the first CI run after merge — the new
  verify steps will be the canary.

### No behavioural changes to the game itself

- `LaunchArgs.cs`, `SessionContext.cs`, menus, theme, autoloads —
  all unchanged. Same UI, same handoff protocol, same in-memory session
  state.
- This is exclusively a release-pipeline fix.

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
