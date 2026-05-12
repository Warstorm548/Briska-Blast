# Dev Tools

## Launcher Branch Channels

The launcher (`launcher/src/updater/branches/`) manages three release tracks:

| Channel | Visibility | Description |
|---|---|---|
| `stable` | Public | Official release builds |
| `experimental` | Opt-in | Testing builds not yet in stable |
| `dev` | Hidden | Dev-only; ships and installs developer tools |

## Dev Channel

The `dev` channel is not visible in the launcher UI unless the logged-in account has the dev flag set server-side. When active, the updater downloads and installs the dev tool files alongside the game — these files do not exist on stable or experimental installs at all.

Stable and experimental builds have no dev tool files present on disk. The separation is at the file distribution level, not a runtime flag or compile-time gate.
