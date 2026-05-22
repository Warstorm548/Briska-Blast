//! Hardcoded data driving v1 UI. Anything that will later come from a file
//! or network is sourced here. When the I/O slices land, this file is the
//! single replacement point.

use crate::channel::Channel;
use crate::identity::{ChannelCreds, Identity};
use std::collections::BTreeMap;

pub fn mock_identity() -> Identity {
    let mut channels = BTreeMap::new();
    channels.insert(
        Channel::Stable,
        ChannelCreds {
            player_id: "0000007".into(),
            secret_token: "deadbeef".into(),
        },
    );
    channels.insert(
        Channel::Ea,
        ChannelCreds {
            player_id: "0000003".into(),
            secret_token: "cafef00d".into(),
        },
    );
    // Dev creds exist locally even when the user isn't flagged for dev UI.
    channels.insert(
        Channel::Dev,
        ChannelCreds {
            player_id: "0000042".into(),
            secret_token: "f00dbabe".into(),
        },
    );
    Identity {
        username: "BlastQueen99".into(),
        channels,
    }
}

/// v1 mock: an unflagged user — dev row hidden everywhere.
pub const VISIBLE_CHANNELS: &[Channel] = &[Channel::Stable, Channel::Ea];

pub const BRANCH_UPDATES_AVAILABLE: &[Channel] = &[Channel::Stable, Channel::Ea];

// Launcher-update availability is no longer mocked — it comes from
// `updater::check_for_update` against GitHub Releases on boot. The mock
// constants `LAUNCHER_UPDATE_AVAILABLE` / `LAUNCHER_AVAILABLE_VERSION` that
// lived here through v0.2.0 are gone; `AppState` defaults the corresponding
// fields to empty / false until the check returns.

pub const MOCK_PROGRESS_PERCENT: u8 = 35;
