//! Hardcoded data driving the still-stubbed parts of the UI. Identity,
//! visible channels, and the "Updates available" banner are no longer
//! mocked — those flow from `identity::load()`, per-launch
//! `server_api::register` responses, and the per-launch `latest_release`
//! fan-out (see `app::boot` and `app::recompute_branch_updates_available`).
//! Only the progress bar remains stubbed until Stage 6 wires it up.

// Launcher-update availability is no longer mocked — it comes from
// `updater::check_for_update` against GitHub Releases on boot. The mock
// constants `LAUNCHER_UPDATE_AVAILABLE` / `LAUNCHER_AVAILABLE_VERSION` that
// lived here through v0.2.0 are gone; `AppState` defaults the corresponding
// fields to empty / false until the check returns. `BRANCH_UPDATES_AVAILABLE`
// was retired in v0.6.0 — `recompute_branch_updates_available` in `app.rs`
// derives the list from real installed vs. available versions.

pub const MOCK_PROGRESS_PERCENT: u8 = 35;
