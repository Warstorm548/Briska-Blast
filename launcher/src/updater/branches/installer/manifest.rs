//! On-disk manifests for an installed channel slot.
//!
//! Two JSON files describe an install dir:
//! * `installed.json` (`InstalledManifest`) — written by the launcher after a
//!   successful extract; records version / channel / executable so future
//!   boots can identify the installed version without re-querying GitHub.
//! * `files.json` (`FilesManifest`) — generated in CI and shipped *inside* the
//!   release archive; the per-file size + sha256 map consumed by `verify`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const MANIFEST_FILENAME: &str = "installed.json";

/// On-disk record of what's currently installed in a channel's install dir.
/// Schema is stable across launcher versions; new optional fields only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledManifest {
    /// Semver string matching the GitHub release tag's version component.
    pub version: String,
    /// Channel name, lowercase (`stable` / `ea` / `dev`).
    pub channel: String,
    /// RFC3339 timestamp of when this slot was installed.
    pub installed_at: String,
    /// Path of the game executable RELATIVE to the install dir. The Godot
    /// export drops the binary either at the top level or nested one
    /// directory deep depending on how the artifact was packaged.
    pub executable: String,
}

pub const FILES_MANIFEST_FILENAME: &str = "files.json";

/// The only `files.json` schema version this launcher understands. CI writes
/// `"schema": 1`; `files_manifest` rejects anything else.
pub const FILES_MANIFEST_SCHEMA: u32 = 1;

/// Build-time per-file integrity manifest, generated in CI and shipped *inside*
/// the release archive as `files.json`. Read by `verify_install` to confirm
/// every shipped file is present, the right size, and (deep pass) the right
/// bytes. Distinct from `installed.json` (which the launcher writes post-extract
/// and records version/channel/exe). The CI generator emits `files.json` last,
/// so it never lists itself; `installed.json` and `saves/` are likewise absent
/// from the map and therefore ignored by verify.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesManifest {
    /// Schema version so a future shape change is detectable. Currently `1`.
    pub schema: u32,
    /// Relative path (forward-slash separated, relative to the install dir) →
    /// expected size + sha256.
    pub files: BTreeMap<String, FileEntry>,
}

/// One file's expected size and lowercase-hex SHA-256, as recorded at build time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub size: u64,
    pub sha256: String,
}

/// Read `<install_dir>/installed.json`. `Ok(None)` if the file is missing
/// (channel is not installed). Parse errors bubble up so callers can flag a
/// corrupted install rather than silently treating it as a fresh slot.
pub async fn installed_manifest(install_dir: &Path) -> Result<Option<InstalledManifest>, String> {
    let path = install_dir.join(MANIFEST_FILENAME);
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("manifest parse: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("manifest read: {e}")),
    }
}

/// Read `<install_dir>/files.json`. `Ok(None)` when absent — an older install
/// packaged before per-file manifests existed, in which case `verify_install`
/// falls back to the cheap exe-exists check. Parse errors bubble up so a
/// corrupted manifest is reported rather than silently treated as "no manifest".
pub async fn files_manifest(install_dir: &Path) -> Result<Option<FilesManifest>, String> {
    let path = install_dir.join(FILES_MANIFEST_FILENAME);
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => {
            let m: FilesManifest =
                serde_json::from_str(&s).map_err(|e| format!("files manifest parse: {e}"))?;
            // Compatibility gate: refuse a manifest written by a future,
            // incompatible schema rather than silently trusting fields whose
            // meaning may have changed.
            if m.schema != FILES_MANIFEST_SCHEMA {
                return Err(format!(
                    "files manifest schema {} is unsupported (this launcher understands {})",
                    m.schema, FILES_MANIFEST_SCHEMA
                ));
            }
            Ok(Some(m))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("files manifest read: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A files.json from a future, incompatible schema is rejected rather than
    /// trusted (compatibility gate).
    #[tokio::test]
    async fn files_manifest_rejects_unknown_schema() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(FILES_MANIFEST_FILENAME),
            br#"{"schema": 2, "files": {}}"#,
        )
        .unwrap();
        assert!(files_manifest(tmp.path()).await.is_err());
    }
}
