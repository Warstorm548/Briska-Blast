//! Per-channel game-files install / update pipeline.
//!
//! Two responsibilities, split across two submodules:
//! * `github.rs` — discovery. Lists GitHub Releases, filters by `game-v`
//!   prefix + channel suffix, returns the highest semver.
//! * `installer.rs` — execution. Picks the platform asset, streams the
//!   download, extracts the archive, writes `installed.json`.
//!
//! Dev gating: every entry point that touches `Channel::Dev` MUST be guarded
//! by `state.dev_flag == true` at the caller site. Unflagged users must not
//! reach this code path for dev — see foundation §3 / Stage 3 plan.

pub mod github;
pub mod installer;

pub use github::{latest_release, GameRelease};
pub use installer::{download_and_install, InstallResult};
