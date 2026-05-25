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

## NSIS Installer Script

The launcher's Windows installer lives at `tools/installer/launcher.nsi`. It is
the only NSIS script in the project — the game itself is not installed by NSIS.
The game is exported by `release-client.yml` (Godot → `BriskaBlast.exe` + `.pck`,
packaged as per-channel `tar.gz`/`zip`) and installed by the launcher's Layer-2
downloader, so the launcher is the sole game-install path.

CI builds the installer with the version passed at build time via
`-DVERSION=x.y.z` on the `makensis` command line — `release-launcher.yml`
invokes `makensis "-DVERSION=${VERSION}"`. The `-D` form is used (not
`/DVERSION=…`) because Git Bash on Windows rewrites `/…` args as POSIX paths;
NSIS itself accepts both `-D` and `/D`.
