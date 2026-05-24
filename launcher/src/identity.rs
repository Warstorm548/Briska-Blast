//! Identity file schema. See docs/launcher/launcher-foundation.md §2.
//! v1 schema; persisted at `paths::identity_path()` next to the binary.

use crate::channel::Channel;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub username: String,
    pub channels: BTreeMap<Channel, ChannelCreds>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChannelCreds {
    /// Canonical zero-padded counter id (9 digits for newly-issued ids; older
    /// 7-digit ids issued before the width bump are accepted unchanged).
    pub player_id: String,
    /// Hex; never displayed in UI. Redacted in Debug output so accidental
    /// `tracing::debug!(?identity, …)` calls cannot leak the token.
    pub secret_token: String,
    /// Per-channel game install directory chosen by the user during the
    /// install-location modal (`<picked_root>/<channel.dir_name()>/`).
    /// `None` until the channel's game is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_location: Option<PathBuf>,
    /// Game version currently installed at `install_location`. Mirrors the
    /// `version` field of `installed.json` written by the installer. Used
    /// by Stage 4's update-check to detect drift against GitHub Releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
}

impl ChannelCreds {
    /// Build a creds row from a fresh `/register` response. install_location
    /// and installed_version stay None until the game is installed for this
    /// channel (Stage 3 install flow).
    pub fn from_register(player_id: String, secret_token: String) -> Self {
        Self {
            player_id,
            secret_token,
            install_location: None,
            installed_version: None,
        }
    }
}

impl fmt::Debug for ChannelCreds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelCreds")
            .field("player_id", &self.player_id)
            .field("secret_token", &"<redacted>")
            .field("install_location", &self.install_location)
            .field("installed_version", &self.installed_version)
            .finish()
    }
}

/// Read `data/identity.json` from the install dir. `Ok(None)` = first-run
/// (file missing). `Err(_)` = real I/O / parse failure; the boot flow treats
/// this the same as first-run (fresh registration) rather than crashing, so
/// a corrupted file is self-healing on next launch.
pub fn load() -> Result<Option<Identity>, String> {
    let path = paths::identity_path().map_err(|e| e.to_string())?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("identity parse: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Atomically persist `identity.json`: write to a sibling `.tmp` file, then
/// rename. Rename is atomic on POSIX and on NTFS, so a torn file is never
/// observable. On Unix, the final file is chmod'd to 0600.
pub fn save(id: &Identity) -> Result<(), String> {
    let path = paths::identity_path().map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");

    let json = serde_json::to_vec_pretty(id).map_err(|e| e.to_string())?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&json).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    }

    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}
