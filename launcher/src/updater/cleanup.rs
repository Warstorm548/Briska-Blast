//! Best-effort removal of leftover self-update artifacts from previous runs.
//!
//! * Windows: `self_replace` drops `.<stem>.<rand>.__relocated__.exe` /
//!   `.<stem>.<rand>.__selfdelete__.exe` next to the running exe and relies on
//!   a helper process to clean them up. If the helper is killed (AV, crash,
//!   race with our `process::exit`) the orphans linger in the install dir.
//! * macOS: the whole-bundle swap leaves the previous app as a sibling
//!   `.<name>.app.old-<stamp>` dir (the updating process was running from it,
//!   so it couldn't delete it) plus, on a crash, a `.<name>.app.staging-*`.
//! * Linux (AppImage): a crashed download can leak a
//!   `.<name>.AppImage.staging-*` file next to the AppImage.
//!
//! Run once at startup to mop all of these up.

use std::path::Path;

const ARTIFACT_SUFFIXES: &[&str] = &[
    ".__relocated__.exe",
    ".__selfdelete__.exe",
    ".__temp__.exe",
];

pub fn cleanup_stale_update_artifacts() {
    let Ok(current) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = current.parent() else {
        return;
    };
    let Some(stem) = current.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    cleanup_in(dir, stem);

    #[cfg(target_os = "macos")]
    if let Some(bundle) = super::github::bundle_root(&current) {
        if let (Some(parent), Some(name)) = (
            bundle.parent(),
            bundle.file_name().and_then(|n| n.to_str()),
        ) {
            cleanup_bundle_leftovers_in(parent, name);
        }
    }

    #[cfg(target_os = "linux")]
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        let path = std::path::PathBuf::from(appimage);
        if let (Some(dir), Some(name)) = (
            path.parent(),
            path.file_name().and_then(|n| n.to_str()),
        ) {
            cleanup_appimage_staging_in(dir, name);
        }
    }
}

/// Remove `.<bundle_name>.old-*` / `.<bundle_name>.staging-*` sibling dirs
/// left by the macOS whole-bundle swap. The live `<bundle_name>` has no
/// leading dot and can never match.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn cleanup_bundle_leftovers_in(parent: &Path, bundle_name: &str) {
    let prefix = format!(".{bundle_name}.");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if !rest.starts_with("old-") && !rest.starts_with("staging-") {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => tracing::info!(path = %path.display(), "removed stale bundle-swap leftover"),
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not remove stale bundle-swap leftover"),
        }
    }
}

/// Remove `.<appimage_name>.staging-*` files left next to the AppImage by a
/// crashed download (a completed swap leaves nothing behind).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn cleanup_appimage_staging_in(dir: &Path, appimage_name: &str) {
    let prefix = format!(".{appimage_name}.staging-");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(path = %path.display(), "removed stale AppImage staging file"),
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not remove stale AppImage staging file"),
        }
    }
}

fn cleanup_in(dir: &Path, stem: &str) {
    let prefix = format!(".{stem}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        if !ARTIFACT_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(path = %path.display(), "removed stale self-update artifact"),
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not remove stale self-update artifact"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn cleans_only_matching_artifacts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let stem = "briskablast-launcher";

        let relocated = dir.join(
            ".briskablast-launcher.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.__relocated__.exe",
        );
        let selfdelete = dir.join(
            ".briskablast-launcher.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.__selfdelete__.exe",
        );
        let temp =
            dir.join(".briskablast-launcher.cccccccccccccccccccccccccccccccc.__temp__.exe");

        let real_exe = dir.join("briskablast-launcher.exe");
        let dotfile_other_ext = dir.join(".briskablast-launcher.something.txt");
        let other_app =
            dir.join(".other-tool.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.__relocated__.exe");

        for p in [
            &relocated,
            &selfdelete,
            &temp,
            &real_exe,
            &dotfile_other_ext,
            &other_app,
        ] {
            fs::write(p, b"").unwrap();
        }

        cleanup_in(dir, stem);

        assert!(!relocated.exists(), "relocated should be deleted");
        assert!(!selfdelete.exists(), "selfdelete should be deleted");
        assert!(!temp.exists(), "temp should be deleted");
        assert!(real_exe.exists(), "real exe must survive");
        assert!(
            dotfile_other_ext.exists(),
            "non-artifact dotfile must survive"
        );
        assert!(other_app.exists(), "other-app artifact must survive");
    }

    #[test]
    fn cleans_only_matching_bundle_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let name = "BriskaBlast Launcher.app";

        let old = dir.join(".BriskaBlast Launcher.app.old-20260704T120000.000Z");
        let staging = dir.join(".BriskaBlast Launcher.app.staging-abc");
        let live = dir.join("BriskaBlast Launcher.app");
        let other = dir.join(".Other.app.old-20260704T120000.000Z");
        for p in [&old, &staging, &live, &other] {
            fs::create_dir(p).unwrap();
        }
        // A dot-prefixed FILE with a matching name must survive (dirs only).
        let stray_file = dir.join(".BriskaBlast Launcher.app.old-notadir");
        fs::write(&stray_file, b"").unwrap();

        cleanup_bundle_leftovers_in(dir, name);

        assert!(!old.exists(), "old bundle should be deleted");
        assert!(!staging.exists(), "staging bundle should be deleted");
        assert!(live.exists(), "live bundle must survive");
        assert!(other.exists(), "other app's leftover must survive");
        assert!(stray_file.exists(), "non-dir entry must survive");
    }

    #[test]
    fn cleans_only_matching_appimage_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let name = "briskablast-launcher-0.20.0-linux.AppImage";

        let staging = dir.join(".briskablast-launcher-0.20.0-linux.AppImage.staging-abc");
        let live = dir.join(name);
        let other = dir.join(".other.AppImage.staging-abc");
        for p in [&staging, &live, &other] {
            fs::write(p, b"").unwrap();
        }

        cleanup_appimage_staging_in(dir, name);

        assert!(!staging.exists(), "staging file should be deleted");
        assert!(live.exists(), "live AppImage must survive");
        assert!(other.exists(), "other AppImage's staging must survive");
    }
}
