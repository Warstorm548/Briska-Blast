//! Hardcoded data driving the still-stubbed parts of the UI. Identity +
//! visible channels are no longer mocked — those flow from
//! `identity::load()` and per-launch `server_api::register` responses (see
//! `app::boot`). This file only holds the few remaining bits of unwired UI.

use crate::channel::Channel;

pub const BRANCH_UPDATES_AVAILABLE: &[Channel] = &[Channel::Stable, Channel::Ea];

// Launcher-update availability is no longer mocked — it comes from
// `updater::check_for_update` against GitHub Releases on boot. The mock
// constants `LAUNCHER_UPDATE_AVAILABLE` / `LAUNCHER_AVAILABLE_VERSION` that
// lived here through v0.2.0 are gone; `AppState` defaults the corresponding
// fields to empty / false until the check returns.

pub const MOCK_PROGRESS_PERCENT: u8 = 35;
