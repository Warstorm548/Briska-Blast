//! Launcher self-update. GitHub Releases → rename-trick binary swap.
//!
//! Tag schema this module consumes: `launcher-v<semver>` (e.g.
//! `launcher-v0.3.0-dev.1`). Prefix isolates launcher tags from the server's
//! own `v*.*.*-dev.N` tag stream and lets `self_update` filter releases on a
//! single string match.
//!
//! Empty sibling subdirs `branches/`, `downloader/`, `patcher/` are placeholders
//! for the future per-channel game-files updater — out of scope on this slice.

pub use cleanup::cleanup_stale_update_artifacts;
pub use github::{check_for_update, run_self_update, UpdateCheckOutcome};

mod cleanup;
mod github;
