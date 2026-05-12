# GitHub Actions Workflows

All workflows live in `.github/workflows/`. They are currently **disabled** (manual trigger only) while active development is underway. To re-enable a workflow, replace its `on: workflow_dispatch:` block with the commented-out `push`/`pull_request` triggers at the top of each file.

## CI Workflows

Run lint, build, and tests for each component.

| Workflow | File | Runs On |
|---|---|---|
| CI — Client | `ci-client.yml` | ubuntu-latest + windows-latest |
| CI — Launcher | `ci-launcher.yml` | ubuntu-latest + windows-latest |
| CI — Server | `ci-server.yml` | ubuntu-latest |

**Rust CI steps** (client + launcher): format check → clippy → build → test

**Go CI steps** (server): gofmt check → vet → build → test

## Release Workflows

Build and package distributable files for each component, then publish a GitHub Release.

| Workflow | File | Linux Output | Windows Output |
|---|---|---|---|
| Release — Client | `release-client.yml` | AppImage + tar.gz | `.exe` installer |
| Release — Launcher | `release-launcher.yml` | AppImage + tar.gz | `.exe` installer |
| Release — Server | `release-server.yml` | tar.gz | `.zip` |

To trigger a release, push a tag matching `v*.*.*` (once the trigger is re-enabled), or run the workflow manually from the GitHub Actions UI.

## Tooling

All tools are open source and AGPL-3.0 compatible.

| Tool | License | Purpose |
|---|---|---|
| `dtolnay/rust-toolchain` | MIT | Rust toolchain setup |
| `Swatinem/rust-cache@v2` | MIT | Cargo registry + target dir caching |
| `actions/setup-go@v5` | MIT | Go setup with module cache |
| NSIS (`makensis`) | Zlib | Windows `.exe` installer builder |
| `linuxdeploy` + `appimagetool` | MIT | Linux AppImage builder |
| `softprops/action-gh-release` | MIT | Publishes GitHub releases with assets |

## NSIS Installer Scripts

NSIS installer templates for the client and launcher live in `tools/installer/`:
- `tools/installer/client.nsi`
- `tools/installer/launcher.nsi`

Each script has TODO comments marking what needs to be filled in before the first release (icon path, version number). The version can be passed at build time via `/DGAME_VERSION=x.y.z` on the `makensis` command line.
