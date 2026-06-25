# Plan: Lobby panel styling + percentage scaling (branch 1), then server-relayed lobby chat (branch 2)

## Context

The Session Lobby (`client/src/ui/menus/SessionLobby.tscn` + `.cs`) currently has two
problems and one unfinished feature:

1. **Panels look undefined.** `MenuTheme.tres` themes only `Button` and `LineEdit`. The
   `PanelContainer` nodes (LeftPanel, RightPanel, ChatBox) fall back to Godot's default
   gray panel, so they read as flat/blurry rather than as deliberate framed areas. The
   user wants them to have a "more bordered, defined look" like the launcher's boxed
   panels, **but using the game's existing blue/cyan theme** (decision below: *game-native
   rounded* — translucent dark-navy fill + 2px light-blue border + 8px corners + subtle
   cyan glow, matching the existing buttons/inputs).

2. **Layout is fixed-pixel, not proportional.** The scene is authored against a
   2560×1440 base (`project.godot`: `stretch/mode="canvas_items"`, `aspect="expand"`),
   and the side panels use absolute top-anchored offsets (e.g. `RightPanel.offset_bottom
   = 1300`, only ~20px above the bottom buttons). The user wants the lobby to scale **by
   percentage of the screen** so every field stays visible on any resolution/aspect.

3. **Chat is stubbed but dead.** `%ChatLog` (RichTextLabel, bbcode) and `%ChatInput`
   (LineEdit) exist in the scene but aren't wired in `SessionLobby.cs`. The follow-up
   branch wires them to a **server-relayed** chat so all players stay in sync, with the
   **Enter key** sending the message.

Work is split across two branches, per the user's request. **Branch 1 is implemented
first**; branch 2 branches off branch 1 afterward.

---

## Branch 1 — `feat/lobby-panel-styling-scaling` (off `dev`)

Visual + layout only. No chat behavior.

### A. Bordered/defined panels — `client/src/ui/theme/MenuTheme.tres`

Add panel styling to the existing theme (keeps the current blue/cyan family used by the
button/lineedit styleboxes already in the file):

- New `StyleBoxFlat` for the **outer** panels — bg `Color(0.06, 0.12, 0.26, 0.88)`
  (translucent navy, sibling of the `lineedit_normal` bg `0.05,0.1,0.22`), `border_width
  = 2` all sides, `border_color = Color(0.25, 0.5, 0.85, 1)` (same light-blue as
  `Button` normal), `corner_radius = 8` all corners, plus a subtle glow `shadow_color =
  Color(0.3, 0.85, 1, 0.18)`, `shadow_size = 8`. Register as `PanelContainer/styles/panel`
  so LeftPanel/RightPanel pick it up automatically.
- New `StyleBoxFlat` for **inner/recessed** boxes (roster box + chat box) — darker bg
  `Color(0.03, 0.07, 0.16, 0.9)`, 2px border in a dimmer blue `Color(0.2, 0.4, 0.7, 1)`,
  8px corners, no glow. Register as a **theme type variation**:
  `InnerPanel/base_type = "PanelContainer"` + `InnerPanel/styles/panel = <inner box>`.

### B. Scene structure + percentage anchors — `client/src/ui/menus/SessionLobby.tscn`

- **Wrap the roster** (`PlayerSlots`) in a new `PanelContainer` "RosterBox" with
  `theme_type_variation = "InnerPanel"` (+ a small inner `MarginContainer`), so the
  roster reads as a recessed inner box like the mockup. Set `ChatBox`'s
  `theme_type_variation = "InnerPanel"` too. LeftPanel/RightPanel keep the default
  `PanelContainer` (outer) style.
- **Convert the two side panels from fixed offsets to fractional anchors** (zero or tiny
  pixel insets), so they hold their proportion of the screen:
  - `LeftPanel`: `anchor_left ≈ 0.03`, `anchor_right ≈ 0.30`, `anchor_top ≈ 0.14`,
    `anchor_bottom ≈ 0.86`.
  - `RightPanel`: `anchor_left ≈ 0.70`, `anchor_right ≈ 0.97`, `anchor_top ≈ 0.14`,
    `anchor_bottom ≈ 0.86`.
  - Title stays top-centered; bottom buttons (`StartSessionButton`,
    `CancelSessionButton`) stay bottom-anchored but live in the band below `0.86`, so
    they can no longer overlap RightPanel. Convert their horizontal offsets to fractional
    anchors for consistency.
- **Guarantee content fits** inside the now-proportional RightPanel: let the roster box
  size to its content and give `ChatBox` `size_flags_vertical = 3` (expand) with a
  reduced `custom_minimum_size` (e.g. height ~160 base px instead of 320) and
  `ChatLog.size_flags_vertical = 3`, so roster + chat share remaining height and the chat
  log shrinks rather than pushing `ChatInput` off-panel. Internal padding stays on the
  existing `MarginContainer`s (panel styleboxes use 0 content margins to avoid double
  padding). Font sizes are left as-is — `canvas_items` stretch scales them with the
  viewport.

### C. Version + changelog

- Bump `client/project.godot` `config/version` `0.12.1` → **`0.13.0`** (minor: UI
  feature). `GameVersion.Current` reads this automatically.
- Add a `## [0.13.0]` entry to `GameChangeLog.md` describing the bordered lobby panels +
  percentage-based responsive layout.
- Dev release tag will be `game-v0.13.0-dev.1` (per `docs/dev/release-tagging.md`).

---

## Branch 2 — `feat/lobby-chat-relay` (off `feat/lobby-panel-styling-scaling`)

Server-relayed lobby chat over the **signaling WebSocket** (the WebRTC mesh isn't built
until the game starts, so the lobby must use the signaling channel). Follows the existing
`ReportScore → broadcast(ScoreUpdate)` pattern exactly.

### Server (Rust)

- `server/src/signaling/protocol.rs`: add `ClientMsg::SendChat { text: String }` and
  `ServerMsg::ChatMessage { from: String, username: String, text: String }`.
- `server/src/signaling/ws/frame.rs`: handle `SendChat` — trim/ignore empty, cap length
  (e.g. 500 chars), resolve the sender's username (reuse the `usernames` map already
  fetched at identify / `fetch_usernames` in `server/src/api/mod.rs`), then
  `signal_hub.broadcast(code, ServerMsg::ChatMessage { from, username, text }, None)`
  (broadcast to **all incl. sender** so every client renders identical, server-ordered
  history). `from` is the server-attested authenticated `player_id` — never client-supplied.
- Bump `server/Cargo.toml` `0.15.0` → **`0.16.0`**; add `## [0.16.0]` to `ServerChangeLog.md`.

### Client (C#)

- `client/src/net/Dto.cs`: add a `ChatMessage` DTO record (`from`, `username`, `text`).
- `client/src/net/SignalingClient.cs`: add `SendChatMessage(string text)` (serialize
  `{"type":"send_chat","text":...}`) and parse incoming `chat_message` → raise a new
  `event Action<string,string,string> ChatMessage` (mirrors how `ScoreUpdate` is wired
  at `SignalingClient.cs:401`).
- `client/src/ui/menus/SessionLobby.cs`: subscribe to `_signaling.ChatMessage` → append
  `[b]{username}[/b]: {text}` to `%ChatLog`; connect `%ChatInput.text_submitted` (fires
  on **Enter**) → `SendChatMessage(text)` + clear input. Remove the placeholder sample
  text from `%ChatLog` in the scene.
- Bump `client/project.godot` version `0.13.0` → **`0.14.0`**; add `## [0.14.0]` to
  `GameChangeLog.md`. Tags: `server-v0.16.0-dev.1`, `game-v0.14.0-dev.1`.

---

## Verification

**Branch 1 (visual/layout):**
- `cd client && dotnet build` (compile check; `.tscn`/`.tres` are data so the build just
  confirms no script regressions).
- Open the project in Godot 4 and run the `SessionLobby` scene (or reach it via host
  flow). Confirm: LeftPanel/RightPanel show the rounded blue-bordered look with the cyan
  glow; roster + chat appear as recessed inner boxes; nothing overlaps the bottom buttons.
- Resize the window across aspect ratios (e.g. 1280×720, 1920×1200, ultrawide, and a
  small/tall window) and confirm **all fields stay fully visible** — session code, mode,
  max-players, all 4 player slots, chat log + input, and both bottom buttons.

**Branch 2 (chat):** with `docker compose up` (server + Redis), run two client instances,
host + join the same session, type in `%ChatInput`, press **Enter**, and confirm the
message appears in both clients' `%ChatLog` with the correct username, in the same order.

## Notes / conventions
- Per repo rules: dev branch work, real version bumps everywhere (not just an Unreleased
  section), semver via the `semver` crate server-side.
- Do **not** run package-wide `cargo fmt` on the launcher/server — local rustfmt drifts;
  hand-match committed style.
- CodeRabbit/CI review is user-triggered — don't poll.
