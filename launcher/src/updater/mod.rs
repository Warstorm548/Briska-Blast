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
pub use relaunch::{spawn_replacement, AFTER_UPDATE_ARG};

/// Bare-binary swap (Windows, and non-bundle/non-AppImage Unix): download the
/// platform asset, extract the executable, swap it with the rename trick.
/// Replaces `self_update`'s opaque `.update()`, which could not report progress.
mod binary_swap;
/// Starting the replacement launcher once the swap is done, and the
/// single-instance handshake that keeps the two processes from cancelling each
/// other out.
mod relaunch;
/// The release type `release_cache::releases` hands back. Re-exported rather
/// than opening `github_client`, which stays private, so callers outside
/// `updater` can name what that public signature already returns.
pub use github_client::Release;

/// Linux AppImage self-update. An AppImage runs from a read-only squashfs
/// mount, so the in-place binary swap is impossible — the outer `.AppImage`
/// file (env `APPIMAGE`) is replaced instead.
#[cfg(target_os = "linux")]
mod appimage;
/// Shared release-asset fetch for every self-update swap path: find the
/// `launcher-v<ver>` release, pick an asset by name suffix, stream it to disk
/// with the same rate-limit handling as the game installer, reporting progress
/// as it goes.
///
/// Was macOS/Linux-only while Windows went through `self_update`'s own opaque
/// `.update()`. Windows now uses this too, because that call exposes no
/// progress callback and the bar has to work on the one platform this project
/// can actually test on.
mod asset_fetch;
mod cleanup;
mod github;
/// macOS whole-bundle self-update. Swapping just the binary inside the .app
/// breaks the ad-hoc signature seal (next launch: "damaged"/killed), so the
/// entire bundle is replaced and re-verified instead.
#[cfg(target_os = "macos")]
mod macos_bundle;
/// The step list one update job is made of, weighted by real release bytes.
/// Drives both the bottom-bar text and the bar's position; the groundwork for
/// phased and per-component updates.
pub mod plan;
/// Owned GitHub Releases list fetch (exposes status + rate-limit headers for the
/// back-off safety net). Private to `updater`; reachable from `branches::github`
/// (a descendant module) and `github` (a sibling).
mod github_client;
/// Shared, conditional release list: one fetch per launch across the self-update
/// check and every channel, revalidated with `If-None-Match` so an unchanged repo
/// costs nothing against the rate limit. Every releases consumer goes through here
/// rather than calling `github_client` directly.
pub mod release_cache;
