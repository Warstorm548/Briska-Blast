//! Per-user data layout. Every launcher-managed file (identity, saves)
//! lives under an OS-correct **per-user** data root resolved by the
//! `directories` crate, NOT next to the binary:
//!
//!   Windows : %APPDATA%\BriskaBlast\data\                       (Roaming)
//!   Linux   : ~/.local/share/briskablast/                       (XDG_DATA_HOME)
//!   macOS   : ~/Library/Application Support/BriskaBlast/data/   (ProjectDirs)
//!
//! This moved out of `<install_dir>/data/` when Layer 1 (the OS installer)
//! began placing the launcher under `C:\Program Files\…`, which a normal
//! user cannot write to. Co-locating data with the binary there fails
//! outright or gets silently shadowed into the UAC VirtualStore — fragile
//! and account-losing. `install_dir()` is retained only to locate the
//! binary (self-update) and to source the one-time migration below; it is
//! no longer the parent of any writable data.

use crate::channel::Channel;
use directories::ProjectDirs;
use std::io;
use std::path::{Path, PathBuf};

/// Directory containing the currently-running launcher binary. Used to
/// locate the binary for self-update and as the *source* of the one-time
/// legacy-data migration — never as the parent of writable data anymore.
pub fn install_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let parent = exe.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "current_exe has no parent directory",
        )
    })?;
    Ok(parent.to_path_buf())
}

/// Per-user data root, created on first call.
///
/// Empty `organization` keeps the Windows path at `%APPDATA%\BriskaBlast\data`
/// (no extra vendor folder), which is exactly the tree the NSIS uninstaller's
/// "keep player data?" prompt targets. The `directories` crate lowercases the
/// application name for the Linux XDG path, giving `~/.local/share/briskablast`.
pub fn data_dir() -> io::Result<PathBuf> {
    let dir = project_dirs()?.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `<data_dir>/identity.json` — the player_id + secret_token store.
pub fn identity_path() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("identity.json"))
}

/// `<data_dir>/saves/<channel>/`. Created on first call. Reserved for the
/// existing `Settings → Game Channel Management → Game Save` button row once
/// it gets a real implementation; exposing the path here is in scope so
/// future code doesn't have to re-derive the layout.
#[allow(dead_code)]
pub fn saves_dir(channel: Channel) -> io::Result<PathBuf> {
    let dir = data_dir()?.join("saves").join(channel.dir_name());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Resolve the `directories` project dirs. Returns an `io::Error` rather than
/// panicking so a headless / no-home environment surfaces as a normal error
/// at the call boundary (matching the rest of this module's `io::Result`).
fn project_dirs() -> io::Result<ProjectDirs> {
    ProjectDirs::from("", "", "BriskaBlast").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no valid home directory for per-user data (ProjectDirs returned None)",
        )
    })
}

/// One-time migration from the pre-Layer-1 co-located layout
/// (`<install_dir>/data/`) into the per-user `data_dir()`. Protects anyone
/// who ran a build from before user data moved off the install dir.
///
/// Returns `Ok(true)` if data was migrated this call, `Ok(false)` if there
/// was nothing to do. Idempotent: once the legacy `identity.json` has been
/// moved out, later calls see no source and no-op. Never overwrites a newer
/// per-user identity — if `data_dir()` already holds an `identity.json`, the
/// migration is skipped entirely so a stale install-dir copy can't clobber it.
pub fn migrate_legacy_data_if_needed() -> io::Result<bool> {
    let legacy_data = install_dir()?.join("data");
    let new_data = data_dir()?;
    migrate_data(&legacy_data, &new_data)
}

/// Core of [`migrate_legacy_data_if_needed`], split out so it can be unit
/// tested against arbitrary source/destination roots (see tests below).
fn migrate_data(legacy_data: &Path, new_data: &Path) -> io::Result<bool> {
    let legacy_identity = legacy_data.join("identity.json");
    // No legacy account on disk → nothing to migrate (also the steady state
    // on every launch after the first post-upgrade one).
    if !legacy_identity.exists() {
        return Ok(false);
    }
    // A per-user identity already exists. The migration must never clobber a
    // newer identity, so we leave both in place and let the per-user copy win
    // (it is what `identity_path()` reads). Bail without touching anything.
    if new_data.join("identity.json").exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(new_data)?;
    // Move every top-level entry (identity.json, saves/, …) from the legacy
    // data dir into the new root. Skip any name already present in the
    // destination so a partially-migrated state can't be overwritten.
    for entry in std::fs::read_dir(legacy_data)? {
        let entry = entry?;
        let from = entry.path();
        let dest = new_data.join(entry.file_name());
        if dest.exists() {
            continue;
        }
        move_path(&from, &dest)?;
    }
    Ok(true)
}

/// Move a file or directory, preferring an atomic rename and falling back to
/// copy-then-delete when the two paths are on different filesystems (e.g. a
/// portable launcher on a removable drive with data under the user's home —
/// `rename(2)` returns `EXDEV` across mount points).
fn move_path(from: &Path, dest: &Path) -> io::Result<()> {
    match std::fs::rename(from, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device (or otherwise un-renameable): deep copy, then
            // remove the source so the operation is still a *move*.
            copy_recursive(from, dest)?;
            if from.is_dir() {
                std::fs::remove_dir_all(from)
            } else {
                std::fs::remove_file(from)
            }
        }
    }
}

/// Recursively copy a file or directory tree. Used only by the cross-device
/// migration fallback above.
fn copy_recursive(from: &Path, dest: &Path) -> io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, dest).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Happy path: a legacy identity + saves tree migrates into an empty
    /// per-user root, the source is emptied (moved, not copied), and the call
    /// reports `true`.
    #[test]
    fn migrates_legacy_data_once() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("install/data");
        let new = tmp.path().join("appdata/data");
        fs::create_dir_all(legacy.join("saves/dev")).unwrap();
        fs::write(legacy.join("identity.json"), b"{\"username\":\"old\"}").unwrap();
        fs::write(legacy.join("saves/dev/slot1.sav"), b"save").unwrap();

        let migrated = migrate_data(&legacy, &new).unwrap();
        assert!(migrated, "first migration should report it moved data");

        // Destination now holds the data…
        assert_eq!(
            fs::read(new.join("identity.json")).unwrap(),
            b"{\"username\":\"old\"}"
        );
        assert!(new.join("saves/dev/slot1.sav").exists());
        // …and it was a move, so the source identity is gone.
        assert!(!legacy.join("identity.json").exists());

        // Idempotent: a second call has no source identity and no-ops.
        let again = migrate_data(&legacy, &new).unwrap();
        assert!(!again, "second migration should be a no-op");
    }

    /// A newer per-user identity must never be clobbered by a stale
    /// install-dir copy: migration bails and leaves the destination untouched.
    #[test]
    fn never_overwrites_newer_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("install/data");
        let new = tmp.path().join("appdata/data");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(legacy.join("identity.json"), b"OLD").unwrap();
        fs::write(new.join("identity.json"), b"NEW").unwrap();

        let migrated = migrate_data(&legacy, &new).unwrap();
        assert!(!migrated, "should refuse to migrate over an existing identity");
        assert_eq!(fs::read(new.join("identity.json")).unwrap(), b"NEW");
    }

    /// No legacy data at all (clean install / portable first run) is a no-op.
    #[test]
    fn no_legacy_data_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("install/data");
        let new = tmp.path().join("appdata/data");
        let migrated = migrate_data(&legacy, &new).unwrap();
        assert!(!migrated);
    }
}
