//! Launcher self-update. GitHub Releases → rename-trick binary swap.
//!
//! Tag schema this module consumes: `launcher-v<semver>` (e.g.
//! `launcher-v0.3.0-dev.1`). Prefix isolates launcher tags from the server's
//! own `v*.*.*-dev.N` tag stream and lets `self_update` filter releases on a
//! single string match.
//!
//! `branches/` is the per-channel game-files install / update pipeline
//! (Stage 3 onward of the launcher game-install plan — distinct from the
//! launcher's own binary self-update). `downloader/` and `patcher/` remain
//! placeholders for the future delta-update / A/B-slot work.

pub mod branches;
pub use cleanup::cleanup_stale_update_artifacts;
pub use github::{check_for_update, run_self_update, UpdateCheckOutcome};

/// Linux AppImage self-update. An AppImage runs from a read-only squashfs
/// mount, so the in-place binary swap is impossible — the outer `.AppImage`
/// file (env `APPIMAGE`) is replaced instead.
#[cfg(target_os = "linux")]
mod appimage;
/// Shared release-asset fetch for the non-`self_update` swap paths (macOS
/// bundle / Linux AppImage): find the `launcher-v<ver>` release, pick an asset
/// by name suffix, stream it to disk with the same rate-limit handling as the
/// game installer.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod asset_fetch;
mod cleanup;
mod github;
/// macOS whole-bundle self-update. Swapping just the binary inside the .app
/// breaks the ad-hoc signature seal (next launch: "damaged"/killed), so the
/// entire bundle is replaced and re-verified instead.
#[cfg(target_os = "macos")]
mod macos_bundle;
/// Owned GitHub Releases list fetch (exposes status + rate-limit headers for the
/// back-off safety net). Private to `updater`; reachable from `branches::github`
/// (a descendant module) and `github` (a sibling).
mod github_client;
