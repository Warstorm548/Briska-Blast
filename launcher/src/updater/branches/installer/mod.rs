//! Download → extract → manifest → verify for a per-channel game-files release.
//!
//! Split across cohesive submodules; this module only wires them together and
//! re-exports the public surface so callers keep using `installer::{...}`:
//! * [`manifest`] — the `installed.json` / `files.json` types + readers.
//! * [`download`] — platform-asset selection + the transactional download /
//!   extract / manifest-write install pipeline.
//! * [`extract`] — archive unpacking + executable resolution (internal).
//! * [`verify`] — file-integrity check against the shipped `files.json`.
//! * [`uninstall`] — teardown with optional timestamped saves backup.

mod download;
mod extract;
mod manifest;
mod uninstall;
mod verify;

// Glob re-exports keep the full `installer::{...}` surface intact (the extract
// submodule is intentionally internal-only). Globs also sidestep the
// unused-import lint on items only consumed inside this module tree.
pub use download::*;
pub use manifest::*;
pub use uninstall::*;
pub use verify::*;
