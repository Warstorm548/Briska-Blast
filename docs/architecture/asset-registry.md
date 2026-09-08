# Asset registry (sprite lookup table)

`client/src/core/SpriteRegistry.cs` is the **single source of truth** for sprite
textures in the game client. It replaces the scattered `res://…` path constants that
used to live in the view, and gives every fast-lookup sprite a **stable number** plus
a **category tag**.

## Why

- One place to declare and find every gameplay sprite.
- A stable integer id (`AssetId`) that never changes once assigned — safe to persist
  or send over the wire later.
- A category tag distinguishing **player-controlled** sprites (paddles, the served
  ball), **system-controlled** static fixtures (the corner barriers),
  **system-handled** random spawns (the ball splitter, loot pickups), and **UI** chrome (the hotbar
  slot frame), so systems can treat each class uniformly — e.g. host spawn-frequency
  settings enumerate only the system spawns via `SpriteRegistry.SystemSpawns()`, which
  the static `SystemControlled` fixtures and `Ui` chrome are deliberately excluded from.

## Shape

- `enum AssetId` — the lookup numbers, counting **upward**; never reorder or reuse a
  number.
- `enum AssetCategory { PlayerControlled, SystemControlled, SystemHandled, Ui }`
  (`SystemControlled` = game-owned but static, e.g. the corner barriers; `SystemHandled`
  = random spawns with a host-tunable cadence, e.g. the ball splitter and loot pickups;
  `Ui` = screen furniture drawn on a
  `CanvasLayer`, e.g. the hotbar slot frame — the other three all answer "who moves this
  thing in the arena", which a UI sprite has no answer to).
- `Entries` — the table ("grid"): one `AssetEntry(id, name, res-path, category)` row
  per sprite.
- API: `GetTexture(AssetId)` (lazy-loads + caches the `Texture2D`), `GetCategory(id)`,
  `SystemSpawns()`.

Registered as the first `[autoload]` in `client/project.godot`, so it's available
before any scene draws (`SpriteRegistry.Instance`).

## Adding a new sprite

1. Drop the PNG under `client/src/assets/sprites/…` and import it in Godot, so the
   `.png.import` sidecar exists — `GD.Load` fails until the editor (or a headless
   `godot --import`) has imported it.
2. Add the next free number to `AssetId`.
3. Add a row to `Entries` with its `res://` path and category.
4. Reference it by id: `SpriteRegistry.Instance.GetTexture(AssetId.YourSprite)`.

For a sprite that needs gameplay behaviour (like a new ball type), also extend the
relevant sim data (e.g. a `BallKind` on `Ball`) — the registry only owns the
**texture + classification**, not the rules.

## Related

- The ball-splitter mechanic that introduced the registry (master ball → 3 BallBT
  split balls, system spawns, double score): see [`extended-mode.md`](extended-mode.md).
- The loot table and the Full Barrier item (the second system-spawn mechanic, and the
  first sprites to serve as both a world pickup and a hotbar icon): see
  [`loot-table-and-barrier.md`](loot-table-and-barrier.md). Note its step-1 warning —
  the headless import rewrites C# indentation.
- Per-channel runtime-cache isolation / `files.json` integrity (a different
  "assets on disk" concern): see
  [`runtime-cache-and-integrity.md`](runtime-cache-and-integrity.md).
