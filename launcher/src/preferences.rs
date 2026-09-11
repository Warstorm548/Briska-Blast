//! Launcher-local UI preferences, persisted at `paths::preferences_path()`
//! under the per-user data root (see `paths.rs`) alongside `identity.json`.
//!
//! Today the file holds exactly one thing: the channel the user last selected,
//! so the next launch comes up where they left off instead of always on Stable.
//! It is deliberately a general *preferences* file rather than a single-purpose
//! one — future remembered UI settings belong here as added fields, not as new
//! files under the data root.
//!
//! **This module never fails.** Every read resolves to a `Channel`, and every
//! write is best-effort. A launcher must boot even when its preferences are
//! garbage, so anything that cannot be trusted — missing, unreadable, corrupt,
//! truncated, or naming a channel this build does not know — resolves to
//! [`FALLBACK`] and, when a bad file is actually present, rewrites it clean so
//! the next launch starts from a good state. That self-heal is what makes a
//! crash (or a kill) during a write recoverable without user action.

use crate::channel::Channel;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The channel every untrustworthy read resolves to. Stable is the safe
/// default: it is the only channel guaranteed visible to every user, so it can
/// never leave the picker showing a row the user is not entitled to.
pub const FALLBACK: Channel = Channel::Stable;

/// On-disk shape of `preferences.json`.
///
/// `#[serde(default)]` on the container is half of what keeps the file
/// forward-compatible: a file written by an older build (field absent) still
/// loads, and unknown fields written by a *newer* build are ignored rather than
/// failing the parse. The other half is on the write side — [`save_at`] merges
/// into the existing document instead of serialising this struct over the top,
/// so an older launcher changing the channel does not delete a newer one's
/// settings. Reading through this struct and writing through the merge is
/// deliberate; do not "simplify" the write to a plain serialise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Last channel the user picked in the left-rail channel box.
    pub selected_channel: Channel,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            selected_channel: FALLBACK,
        }
    }
}

/// What a read of the file found. The Missing/Unusable split exists only so the
/// self-heal can tell a normal first run (no file yet — leave it alone, the
/// first pick creates it) from a genuinely broken file worth overwriting.
#[derive(Debug)]
enum LoadOutcome {
    Loaded(Preferences),
    /// No file yet — first run, or the data dir was wiped.
    Missing,
    /// Present but not usable; the string is the reason, for the log line.
    Unusable(String),
}

/// Last channel the user selected, or [`FALLBACK`].
///
/// Infallible **by signature** on purpose: returning a bare `Channel` rather
/// than a `Result` or `Option` means no call site can forget to apply the
/// fallback, so the fail-safe cannot be bypassed by a future caller.
pub fn load_selected_channel() -> Channel {
    let path = match paths::preferences_path() {
        Ok(p) => p,
        Err(e) => {
            // No data dir (headless / no home). Not worth failing a launch over.
            tracing::warn!(error = %e, "no data dir for preferences — using {FALLBACK}");
            return FALLBACK;
        }
    };
    resolve(&path, load_at(&path))
}

/// Persist `channel` as the remembered selection. Best-effort: a failure is
/// logged and swallowed, never surfaced to the UI. Losing the *next* launch's
/// starting channel is not worth interrupting the user mid-click over, and the
/// in-memory selection they just made is unaffected either way.
pub fn save_selected_channel(channel: Channel) {
    let path = match paths::preferences_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "no data dir — not persisting channel selection");
            return;
        }
    };
    if let Err(e) = save_at(&path, channel) {
        tracing::warn!(error = %e, "failed to persist preferences.json (non-fatal)");
    }
}

/// Read and parse the file. Split from [`load_selected_channel`] so the tests
/// can drive it against a temp path instead of the real data root — the same
/// shape `ratelimit::load_at` uses.
fn load_at(path: &Path) -> LoadOutcome {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LoadOutcome::Missing,
        Err(e) => return LoadOutcome::Unusable(e.to_string()),
    };
    match serde_json::from_str::<Preferences>(&raw) {
        Ok(prefs) => LoadOutcome::Loaded(prefs),
        // Covers every "present but wrong" case with one arm: truncated JSON,
        // an empty file, and a `selected_channel` naming something that is not
        // one of this build's channels (`Channel` has no catch-all variant, so
        // an unknown name is a parse error, not a silently-accepted value).
        Err(e) => LoadOutcome::Unusable(format!("preferences parse: {e}")),
    }
}

/// Turn a load outcome into the channel to select, repairing a bad file on the
/// way through. A *missing* file is left alone — that is the steady state of a
/// first run, and writing one at boot would be a pointless disk touch, since
/// the user's first pick creates it anyway.
fn resolve(path: &Path, outcome: LoadOutcome) -> Channel {
    match outcome {
        LoadOutcome::Loaded(prefs) => prefs.selected_channel,
        LoadOutcome::Missing => FALLBACK,
        LoadOutcome::Unusable(reason) => {
            tracing::warn!(
                reason,
                path = %path.display(),
                "preferences.json unusable — resetting it to {FALLBACK}"
            );
            // Self-heal, best-effort. If even this write fails the next launch
            // simply repeats the same fallback, so a read-only data dir
            // degrades to "always Stable" rather than to a broken launcher.
            if let Err(e) = save_at(path, FALLBACK) {
                tracing::warn!(error = %e, "could not rewrite a clean preferences.json");
            }
            FALLBACK
        }
    }
}

/// Write `channel` into the file, **merging** rather than replacing.
///
/// The merge is what makes the forward compatibility on the read side mean
/// anything: a field written by a newer launcher must survive a channel change
/// made by an older one, and serialising a whole `Preferences` over the top
/// would silently drop every key this build does not know about. With one field
/// in the struct that is theoretical, but it stops being theoretical the moment
/// a second setting lands, and it is far easier to get right now than to
/// remember later.
///
/// Atomic via the shared [`paths::write_atomic`] helper — a uuid-suffixed
/// sibling tmp then rename — so a torn file is never observable by the next
/// launch's read, and this file keeps the same durability property every other
/// remembered file under the data root already has.
fn save_at(path: &Path, channel: Channel) -> Result<(), String> {
    let mut doc = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .filter(serde_json::Value::is_object)
        // Missing, unreadable, or not a JSON object at all: there is nothing
        // worth preserving, so start from a clean one. This is also the shape
        // the self-heal path takes, which is why a corrupt file is replaced
        // outright while a merely *wrong* one keeps its other keys.
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    doc["selected_channel"] = serde_json::to_value(channel).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    paths::write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Drive the real read path against a temp file, exactly as
    /// `load_selected_channel` does once it has resolved the data dir.
    fn read(path: &Path) -> Channel {
        resolve(path, load_at(path))
    }

    #[test]
    fn round_trips_a_saved_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        for channel in Channel::all() {
            save_at(&path, channel).unwrap();
            assert_eq!(read(&path), channel);
        }
    }

    /// First run: no file yet. Falls back, and must NOT create one.
    #[test]
    fn missing_file_is_fallback_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        assert_eq!(read(&path), FALLBACK);
        assert!(
            !path.exists(),
            "a missing preferences file must not be created at boot"
        );
    }

    /// A zero-length file — the classic result of a crash between create and
    /// write. Falls back AND is repaired.
    #[test]
    fn empty_file_is_fallback_and_self_heals() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(&path, b"").unwrap();

        assert_eq!(read(&path), FALLBACK);
        // Repaired in place: the next read is a clean Loaded, not another
        // Unusable, so the bad state does not persist across launches.
        assert!(matches!(load_at(&path), LoadOutcome::Loaded(p) if p.selected_channel == FALLBACK));
    }

    /// Truncated mid-write — a torn file the atomic write is meant to prevent,
    /// but which a pre-existing install could still be carrying.
    #[test]
    fn truncated_json_is_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(&path, b"{\"selected_channel\": \"d").unwrap();
        assert_eq!(read(&path), FALLBACK);
    }

    /// Outright garbage, e.g. a file clobbered by an unrelated writer.
    #[test]
    fn corrupt_bytes_are_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(&path, b"\x00\xff\xfe not json at all").unwrap();
        assert_eq!(read(&path), FALLBACK);
    }

    /// Valid JSON naming a channel this build does not have. This is the case
    /// the user specifically asked about: an incomplete or invalid channel name
    /// must land on Stable rather than being half-accepted.
    #[test]
    fn unknown_channel_name_is_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        for bad in ["banana", "", "DEV", "de"] {
            fs::write(&path, format!("{{\"selected_channel\":\"{bad}\"}}")).unwrap();
            assert_eq!(read(&path), FALLBACK, "channel name {bad:?} must not load");
        }
    }

    /// Valid JSON with the field absent. `#[serde(default)]` fills it, so this
    /// loads cleanly as the fallback rather than counting as corruption.
    #[test]
    fn empty_object_defaults_to_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(&path, b"{}").unwrap();
        assert!(matches!(load_at(&path), LoadOutcome::Loaded(_)));
        assert_eq!(read(&path), FALLBACK);
    }

    /// The write side of forward compatibility, and the regression guard for it:
    /// changing the channel must not delete a setting written by a newer build.
    #[test]
    fn saving_a_channel_preserves_unknown_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(
            &path,
            b"{\"selected_channel\":\"stable\",\"theme\":\"dark\",\"window_w\":1280}",
        )
        .unwrap();

        save_at(&path, Channel::Ea).unwrap();

        assert_eq!(read(&path), Channel::Ea);
        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["selected_channel"], "ea");
        assert_eq!(doc["theme"], "dark", "a newer build's setting must survive");
        assert_eq!(doc["window_w"], 1280);
    }

    /// The flip side: a file with nothing worth keeping is replaced outright,
    /// so corruption cannot survive by being merged into.
    #[test]
    fn saving_over_a_corrupt_file_replaces_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        fs::write(&path, b"\x00\xff not json").unwrap();

        save_at(&path, Channel::Ea).unwrap();

        assert_eq!(read(&path), Channel::Ea);
    }

    /// A file that parses as an object but names a channel this build does not
    /// know is *wrong*, not corrupt — the self-heal resets the channel while
    /// leaving the rest of the user's settings alone.
    #[test]
    fn self_heal_keeps_other_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        let raw = b"{\"selected_channel\":\"banana\",\"theme\":\"dark\"}";
        fs::write(&path, raw).unwrap();

        assert_eq!(read(&path), FALLBACK);

        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["selected_channel"], "stable");
        assert_eq!(doc["theme"], "dark");
    }

    /// Forward compatibility: a file written by a future launcher that remembers
    /// more settings must still yield its channel to this build, untouched.
    #[test]
    fn unknown_extra_fields_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("preferences.json");
        let before = b"{\"selected_channel\":\"ea\",\"theme\":\"dark\",\"window_w\":1280}";
        fs::write(&path, before).unwrap();

        assert_eq!(read(&path), Channel::Ea);
        // And the read must not have "repaired" a perfectly good file, which
        // would silently drop the settings the newer build is relying on.
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
