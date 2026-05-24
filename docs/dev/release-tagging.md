# Release Tag Namespaces

Briska Blast ships three independently-versioned components, each with its
own GitHub Release stream and its own CI workflow. Keep their tag
namespaces distinct — collisions trigger the wrong workflow and stamp the
wrong artifacts.

## Namespaces

| Component | Tag pattern | Workflow | Examples |
|---|---|---|---|
| Server | `v<semver>` (`-ea.N` / `-dev.N` optional) | `.github/workflows/release-server.yml` | `v0.5.1`, `v0.6.0-dev.1`, `v0.6.0-ea.1` |
| Launcher | `launcher-v<semver>` (`-ea.N` / `-dev.N` optional) | `.github/workflows/release-launcher.yml` | `launcher-v0.4.0-dev.1`, `launcher-v1.0.0` |
| Game | `game-v<semver>` (`-ea.N` / `-dev.N` optional) | `.github/workflows/release-client.yml` | `game-v0.2.0-dev.1`, `game-v1.0.0-ea.1`, `game-v1.0.0` |

## Channel rules

For every component, the same three-channel pattern applies:

| Channel | Suffix | Prerelease | Notes |
|---|---|---|---|
| `stable` | (none) | `false` | Public release. |
| `ea` | `-ea.N` | `true` | Early-access opt-in. |
| `dev` | `-dev.N` | `true` | Dev-flagged users only (see [`launcher-foundation.md`](../launcher/launcher-foundation.md) §3). |

`N` is a monotonic build counter within a base version — `game-v0.2.0-dev.1`,
`-dev.2`, `-dev.3`, etc. Bump the base version (`0.2.0` → `0.3.0`) when
promoting to a new feature set; bump `N` for incremental dev/ea iterations.

## Why three separate namespaces

The launcher's `self_update` flow polls GitHub Releases for **its own**
component's tag prefix. If the server used `launcher-v*` or the game used
bare `v*`, the launcher would pick up the wrong release and attempt to
overwrite itself with mismatched binaries.

The previous `release-client.yml` was wired for bare `v*.*.*` (matching
server), which would have collided on every server tag push. The
`game-v*` prefix fixes that.

## When tagging

- Push the tag from a clean commit on `main` (for stable) or the relevant
  feature branch (for `-ea` / `-dev`).
- The matching workflow auto-triggers, builds artifacts, and publishes the
  GitHub Release with `prerelease` set per the channel rules above.
- `workflow_dispatch` is also available on each workflow for manual builds
  during testing — those produce CI artifacts but do **not** publish a
  GitHub Release.

## Coordinated-release ordering

When a release touches more than one component, push the tags in this
order so each downstream component targets only already-published deps:

1. **Server** — `v<semver>(-{ea,dev}.N)?`. Wait for `release-server.yml`
   to finish and the Release to appear; the game and launcher don't
   depend on a server *artifact* but they do depend on whatever protocol
   the new server version expects.
2. **Game** — `game-v<semver>(-{ea,dev}.N)?`. Wait for
   `release-client.yml` to publish the Release; the launcher's per-channel
   `latest_release` discovery (Stage 3+ of the launcher install pipeline)
   reads this Release.
3. **Launcher** — `launcher-v<semver>(-{ea,dev}.N)?`. The launcher's own
   `self_update` flow only looks at `launcher-v*` tags, so its order
   inside this trio is loose — but tagging last is the safe default:
   any user who updates immediately gets the launcher build that knows
   about the just-published server protocol and game release.

If you're cutting just one component (a launcher patch, a game-only
content drop, etc.), this ordering is moot — push the tag whenever.

## Related

- [`devtools.md`](devtools.md) — dev branch and channel visibility rules.
- [`../launcher/launcher-foundation.md`](../launcher/launcher-foundation.md) §7 — launcher update model.
- [`../launcher/launcher-update-and-version-validation.md`](../launcher/launcher-update-and-version-validation.md) — version-enforcement flow.
