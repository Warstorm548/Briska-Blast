//! Identity file schema. See docs/launcher/launcher-foundation.md §2.
//! v1 defines the types; file I/O lands in a later slice.

use crate::channel::Channel;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub username: String,
    pub channels: BTreeMap<Channel, ChannelCreds>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCreds {
    /// Canonical 7-digit, e.g. "0000007".
    pub player_id: String,
    /// Hex; never displayed in UI.
    pub secret_token: String,
}
