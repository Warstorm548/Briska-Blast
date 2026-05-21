# Launcher Changelog

All notable changes to the Briska Blast launcher are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [0.1.0] — 2026-05-21

First versioned launcher build. UI scaffold + data model only — no file
I/O, no network calls, no settings panel, no update logic. Establishes
the package shape, the 5-zone window layout, the identity-file schema
as compile-checked Rust types, and the channel taxonomy. Buttons exist
but only log clicks at debug level.

### Added

- **Foundation design doc** at `docs/launcher/launcher-foundation.md` —
  the spec this code implements. Covers the 5-zone layout, the local
  identity file shape, channel visibility gating (dev hidden behind a
  server-side per-user flag), nine UI state variants, the two-stream
  update model (launcher self-update vs game-files-per-channel), the
  planned A/B install slots successor, server-status panel meanings,
  and explicitly deferred items.

- **Workspace integration** — `launcher` added to the root `Cargo.toml`
  `[workspace] members`. Edition 2021, resolver `"2"` matching `server/`
  and `shared/`.

- **Iced 0.14 application** — synchronous `main()` (Iced owns the async
  runtime via its `tokio` feature). `iced::application(boot, update,
  view).title(...).theme(Theme::Dark).run()` pattern.

- **5-zone layout** (`launcher/src/ui/`):
  - **Top bar** — launcher version (`env!("CARGO_PKG_VERSION")`),
    branch-updates banner, launcher-update banner, gear icon button.
  - **Left rail** — channel picker + server-status dots panel. Dev row
    filtered out when not in `state.visible_channels`.
  - **Center pane** — title + "no menu selected" placeholder. Styled
    with a subtly-darker background and thicker border to mark it as
    the menu-display surface.
  - **Right rail** — username display + Change Name button + per-channel
    Player IDs list (dev row hidden for unflagged users).
  - **Bottom bar** — Update button, progress placeholder (mock 35%
    complete), Play button.

- **Boxed sub-element styling** — each logical sub-element within a
  zone is wrapped in a bordered container so the layout matches the
  hand-drawn foundation mockup at `Example Imgs/Luncher Design.png`.
  Two `container::Style` helpers in `ui/theme.rs`: `bordered` (thin
  1.5px white @ 40% alpha border, no fill) and `menu_pane` (2px @ 55%
  border + 6% white-alpha fill).

- **Data model** (top-level `launcher/src/`):
  - `channel.rs` — `Channel` enum (`Stable` / `Ea` / `Dev`) with serde
    `rename_all = "lowercase"`. `Channel::host()` returns the baked-in
    hostnames matching `client/BriskaBlast.csproj`'s GenerateBuildConfig
    target. `Channel: Ord` by discriminant gives canonical `stable → ea
    → dev` iteration order.
  - `identity.rs` — Serde `Identity { username, channels:
    BTreeMap<Channel, ChannelCreds> }` matching foundation doc §2.
    `BTreeMap` (not `HashMap`) preserves key order on serde roundtrip.
  - `mock.rs` — sole source of v1 fake reality: one shared username
    ("BlastQueen99"), per-channel mock player IDs, visible-channels
    list (`[Stable, Ea]` — unflagged-user mock), update-available
    state, progress percentage.

- **Application glue** (`launcher/src/app.rs`) — `AppState` view-model,
  closed `Message` enum (PlayPressed, UpdatePressed, OpenSettings,
  ChannelPicked, ChangeNamePressed, LauncherUpdatePressed), `update`/
  `view`/`theme`/`title`. `AppState::default()` constructs from
  `mock::*` constants so the v1.x I/O slice has a single replacement
  point.

- **Tracing init** mirroring `server/src/main.rs:26-32` — registry →
  `EnvFilter` (default `launcher=info`) → `fmt::layer`. Every Message
  in `update()` is logged at debug.

- **Channel taxonomy alignment** — `experimental` renamed to `ea`
  across `docs/dev/devtools.md`, `docs/server/server-autoupdate.md`,
  and `readme.md`. Resolves the cross-doc naming inconsistency where
  the launcher doc used `experimental` while the server / client /
  CI already used `ea`.

- **CLAUDE.md** — index row pointing at `launcher-foundation.md` under
  "Where to Find Information."

### Verification

- `cargo check -p launcher` — clean, no warnings (dead-code on
  `Channel::host`/`all` annotated with `#[allow(dead_code)]` until the
  network slice consumes them).
- `cargo build -p launcher` — full debug build in ~47s cold
  (`wgpu` + `winit` + glyph stacks).
- `cargo run -p launcher` on WSL2 + WSLg — window opens cleanly; runs
  uninterrupted for 6-8s under timeout-driven SIGTERM (exit 143, not a
  panic).

### Notes

- v1 buttons are intentionally inert; only `ChannelPicked` mutates
  state. Each click logs at debug via `tracing::debug!(?message, ...)`.
- Iced 0.14 expects `Pixels: From<u32>` or `From<f32>` — `u16` does
  not satisfy. Theme constants are `u32`.
- WSL2 dev environments may need `WINIT_UNIX_BACKEND=x11` or the
  `tiny-skia` feature on `iced` if `wgpu` + Vulkan can't initialize.
  Windows / native Linux not affected.
- Module subdirs `auth/`, `settings/`, `news/`, `networking/`,
  `updater/`, `config/`, `devtools/` keep their `.gitkeep` placeholders
  — no fake stubs. They light up as future slices populate them.
- `channel.rs`, `identity.rs`, `mock.rs` live at top-level `src/` for
  v1; they migrate into `auth/` / `config/` when those modules light
  up.

### Deferred (not in this release)

The following are scoped into future v1.x slices; not in this PR:

- Identity file I/O (read/write `identity.json` at a platform-
  appropriate path).
- Platform-paths crate selection (`dirs` vs `directories`).
- First-launcher-launch parallel reach-out to all three channel
  servers (depends on server endpoint design).
- Dev-flag retrieval from the dev server.
- Settings panel content.
- Update flow logic (game-files update + launcher self-update via the
  `self_update` crate).
- Channel switch confirmation modal.
- HTTP 426 "update required" prompt.
- Button coloring (Update / Play green, progress bar blue/purple) to
  finish matching the mockup.
- A/B install slots — the planned successor to the v1 "refuse to
  launch during update" rule (foundation doc §7).
- WS-ticket auth and TURN credentials delivery
  (`docs/planning/roadmap.md`).
