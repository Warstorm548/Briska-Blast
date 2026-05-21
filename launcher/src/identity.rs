//! Identity file schema. See docs/launcher/launcher-foundation.md §2.
//! v1 defines the types; file I/O lands in a later slice.

use crate::channel::Channel;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub username: String,
    pub channels: BTreeMap<Channel, ChannelCreds>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelCreds {
    /// Canonical 7-digit, e.g. "0000007".
    pub player_id: String,
    /// Hex; never displayed in UI. Redacted in Debug output so accidental
    /// `tracing::debug!(?identity, …)` calls cannot leak the token.
    pub secret_token: String,
}

impl fmt::Debug for ChannelCreds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelCreds")
            .field("player_id", &self.player_id)
            .field("secret_token", &"<redacted>")
            .finish()
    }
}
