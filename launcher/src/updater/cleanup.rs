//! Best-effort removal of leftover `self_replace` artifacts from previous
//! self-updates. On Windows, `self_replace` drops `.<stem>.<rand>.__relocated__.exe`
//! and `.<stem>.<rand>.__selfdelete__.exe` next to the running exe and relies on
//! a helper process to clean them up. If the helper is killed (AV, crash, race
//! with our `process::exit`) the orphans linger in the install dir. Run this
//! once at startup to mop them up.

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
}
