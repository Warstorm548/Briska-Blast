# Dev Tools

## Launcher Branch Channels

The launcher (`launcher/src/updater/branches/`) manages three release tracks:

| Channel | Visibility | Description |
|---|---|---|
| `stable` | Public | Official release builds |
| `ea` | Opt-in | Early access; testing builds not yet promoted to stable |
| `dev` | Hidden | Dev-only; ships and installs developer tools |

## Dev Channel

The `dev` channel is not visible in the launcher UI unless the logged-in account has the dev flag set server-side. When active, the updater downloads and installs the dev tool files alongside the game — these files do not exist on stable or EA installs at all.

Stable and EA builds have no dev tool files present on disk. For these, the separation is at the file distribution level, not a runtime flag or compile-time gate.

## In-game dev tools (compiled, not distributed)

The rule above governs dev tool **files the launcher installs beside the game**. It
cannot govern code compiled *into* the game assembly — there is no file to withhold
short of shipping a second assembly. Those tools are gated twice instead, and both
halves matter:

1. **Compile-time.** `client/BriskaBlast.csproj` defines `DEV_TOOLS` only when
   `ReleaseChannel == dev`. Everything inside `#if DEV_TOOLS` is **absent from ea and
   stable assemblies** — not dead code, not an unreachable branch. Verified by
   building with `-p:ReleaseChannel=stable` and finding no dev symbols in the output.
2. **Runtime.** The tools additionally refuse unless `OS.HasFeature("editor")`. A
   dev-channel export on a tester's machine therefore carries them **inert**.

The first controls what ships; the second controls what runs. Neither is redundant.

### `/` chat commands

Game 0.32.0 added the `chat_command` key (`/`), which opens chat with the slash
pre-typed "so a future dev-tools parser has its prefix". `client/src/dev/DevCommands.cs`
is that parser, hooked into the one place a submitted line passes through
(`ChatPanel.OnSubmitted`, shared by the lobby and the match).

| Command | Effect |
|---|---|
| `/help` | Lists the available commands. |
| `/lb [2-8]` | Toggles the leaderboard demo — fake players whose scores move so the panel, the rank swaps and the glide can be seen without standing up a match. Outside a match it says so rather than doing nothing. |

Replies are written straight into the local `ChatLog` and never touch the socket, so
no peer sees them.

**An unrecognised `/…` line still posts as ordinary chat**, exactly as 0.32.0
documented. The parser consumes only what it recognises — a typo'd command reaching
the session is better than one vanishing into a parser that silently ate it.

`DevCommands` lives in `client/src/dev/` rather than `src/core/` because it reaches a
UI component and Core does not reference UI anywhere else; that layering is worth
keeping clean for code that never ships.
