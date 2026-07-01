//! File-integrity verification against the shipped `files.json`.
//!
//! Reads `installed.json` for the version, then — when the build shipped a
//! `files.json` — checks every listed file is present, the right size, and the
//! right bytes. Falls back to a cheap exe-exists check for installs packaged
//! before per-file manifests existed.

use std::path::{Path, PathBuf};

use super::manifest::{files_manifest, installed_manifest, FilesManifest};

/// True only for a strictly-relative path with no root, drive prefix, or `..`
/// components — so joining it onto the install dir can never escape the tree.
/// Guards `verify_files` against a tampered `files.json` pointing it at files
/// outside the install (e.g. `../../etc/passwd` or `/etc/shadow`).
fn is_safe_relpath(rel: &str) -> bool {
    use std::path::Component;
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Outcome of a Verify File Integrity check. The diagnostic payloads
/// (`ManifestUnreadable`'s string, `ExecutableMissing`'s path) are surfaced
/// via the `?outcome` Debug formatter on the VerifyComplete tracing log;
/// the inline status cell in Settings shows only a short label. Allow
/// dead_code so the fields can be promoted to a hover tooltip later
/// without rewriting the enum.
#[derive(Debug, Clone)]
pub enum VerifyOutcome {
    Ok {
        version: String,
    },
    ManifestMissing,
    ManifestUnreadable(#[allow(dead_code)] String),
    ExecutableMissing {
        #[allow(dead_code)]
        expected: PathBuf,
    },
    /// Deep verify (cheap pass): files listed in `files.json` that are absent or
    /// the wrong size on disk. `count` is the total; `sample` holds up to a few
    /// relpaths for the `?outcome` tracing log / a future hover tooltip.
    FilesMissing {
        count: usize,
        #[allow(dead_code)]
        sample: Vec<String>,
    },
    /// Deep verify (hash pass): files present and the right size, but whose
    /// sha256 doesn't match the manifest — corruption or tampering.
    FilesCorrupted {
        count: usize,
        #[allow(dead_code)]
        sample: Vec<String>,
    },
}

/// Integrity check. Reads `installed.json` for the version, then — when the
/// build shipped a `files.json` — verifies every listed file is present, the
/// right size (cheap pass), and the right bytes (deep sha256 pass on a blocking
/// thread, since the `.pck` is hundreds of MB). Falls back to the historic
/// exe-exists check when `files.json` is absent (installs packaged before
/// per-file manifests).
pub async fn verify_install(install_dir: PathBuf) -> VerifyOutcome {
    let manifest = match installed_manifest(&install_dir).await {
        Ok(Some(m)) => m,
        Ok(None) => return VerifyOutcome::ManifestMissing,
        Err(e) => return VerifyOutcome::ManifestUnreadable(e),
    };

    match files_manifest(&install_dir).await {
        Ok(Some(files)) => verify_files(&install_dir, &files, &manifest.version).await,
        Ok(None) => {
            // Legacy install (no files.json) — fall back to the cheap exe check.
            let exe = install_dir.join(&manifest.executable);
            match tokio::fs::metadata(&exe).await {
                Ok(_) => VerifyOutcome::Ok {
                    version: manifest.version,
                },
                Err(_) => VerifyOutcome::ExecutableMissing { expected: exe },
            }
        }
        Err(e) => VerifyOutcome::ManifestUnreadable(e),
    }
}

/// Two-pass per-file verify against `files.json`. Pass 1 (cheap, async): every
/// listed file is present and its size matches — catches the common breakage
/// (deleted / truncated / half-extracted files) instantly without reading bytes.
/// Pass 2 (deep): sha256 of each survivor matches, run on a blocking thread with
/// chunked reads so a multi-hundred-MB `.pck` doesn't stall the async runtime.
/// Files on disk that aren't in the manifest (e.g. `installed.json`, `saves/`)
/// are ignored — verify only asserts the *manifest's* files.
async fn verify_files(install_dir: &Path, files: &FilesManifest, version: &str) -> VerifyOutcome {
    /// How many failing relpaths to retain for diagnostics (the full count is
    /// always reported; the sample bounds the log/tooltip size).
    const SAMPLE_MAX: usize = 5;

    // Pass 1: presence + size.
    let mut missing: Vec<String> = Vec::new();
    for (rel, entry) in &files.files {
        // Reject unsafe entries (absolute / `..` / drive-prefix) before touching
        // the filesystem — a tampered manifest must not escape the install tree.
        if !is_safe_relpath(rel) {
            missing.push(rel.clone());
            continue;
        }
        let path = install_dir.join(rel);
        let ok = matches!(tokio::fs::metadata(&path).await, Ok(m) if m.len() == entry.size);
        if !ok {
            missing.push(rel.clone());
        }
    }
    if !missing.is_empty() {
        let count = missing.len();
        missing.truncate(SAMPLE_MAX);
        return VerifyOutcome::FilesMissing {
            count,
            sample: missing,
        };
    }

    // Pass 2: sha256. Offloaded to a blocking thread (chunked I/O over GBs).
    let dir = install_dir.to_path_buf();
    let files = files.clone();
    let corrupted = match tokio::task::spawn_blocking(move || {
        let mut bad: Vec<String> = Vec::new();
        for (rel, entry) in &files.files {
            // Same containment guard as pass 1, applied before hashing.
            if !is_safe_relpath(rel) {
                bad.push(rel.clone());
                continue;
            }
            let hashed = hash_file_blocking(&dir.join(rel));
            let ok = matches!(hashed, Ok(ref hex) if hex.eq_ignore_ascii_case(&entry.sha256));
            if !ok {
                bad.push(rel.clone());
            }
        }
        bad
    })
    .await
    {
        Ok(bad) => bad,
        Err(e) => {
            // A join failure means we couldn't confirm integrity — report it as
            // corruption rather than silently passing.
            tracing::warn!(error = %e, "verify hash task failed to join");
            vec!["<hash task failed>".to_string()]
        }
    };

    if corrupted.is_empty() {
        VerifyOutcome::Ok {
            version: version.to_string(),
        }
    } else {
        let count = corrupted.len();
        let mut sample = corrupted;
        sample.truncate(SAMPLE_MAX);
        VerifyOutcome::FilesCorrupted { count, sample }
    }
}

/// SHA-256 a file with a fixed-size buffer (never reads the whole file into
/// memory — the `.pck` can be ~1 GB). Returns lowercase hex. Blocking; call
/// only inside `spawn_blocking`.
fn hash_file_blocking(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::manifest::{
        FileEntry, FilesManifest, InstalledManifest, FILES_MANIFEST_FILENAME, MANIFEST_FILENAME,
    };
    use std::collections::BTreeMap;

    /// Mirror of `hash_file_blocking` for computing the test's expected digests.
    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        format!("{:x}", h.finalize())
    }

    /// Deep verify against a `files.json`: a clean tree passes; a same-size byte
    /// change is caught by the hash pass; a deleted file is caught by the cheap
    /// presence/size pass (before any hashing). Exercises the whole
    /// `verify_install` → `verify_files` path on every platform.
    #[tokio::test]
    async fn verify_install_deep_pass_detects_corruption_and_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        std::fs::write(dir.join("BriskaBlast.x86_64"), b"the-binary-bytes").unwrap();
        std::fs::write(dir.join("game.pck"), b"pack-contents").unwrap();

        let installed = InstalledManifest {
            version: "0.17.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-24T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();

        let mut files = BTreeMap::new();
        for name in ["BriskaBlast.x86_64", "game.pck"] {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            files.insert(
                name.to_string(),
                FileEntry {
                    size: bytes.len() as u64,
                    sha256: sha256_hex(&bytes),
                },
            );
        }
        std::fs::write(
            dir.join(FILES_MANIFEST_FILENAME),
            serde_json::to_vec(&FilesManifest { schema: 1, files }).unwrap(),
        )
        .unwrap();

        // Clean tree → Ok. (Extra unlisted files like installed.json are ignored.)
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::Ok { .. }
        ));

        // Same-length byte change → passes the size pass, fails the hash pass.
        std::fs::write(dir.join("game.pck"), b"pack-CONTENTS").unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesCorrupted { count: 1, .. }
        ));

        // Deleted file → caught by the cheap pass before any hashing.
        std::fs::remove_file(dir.join("game.pck")).unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesMissing { count: 1, .. }
        ));
    }

    /// No `files.json` (an install packaged before per-file manifests) falls
    /// back to the historic exe-exists check rather than erroring.
    #[tokio::test]
    async fn verify_install_without_files_manifest_falls_back_to_exe_check() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("BriskaBlast.x86_64"), b"bin").unwrap();
        let installed = InstalledManifest {
            version: "0.16.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-24T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::Ok { .. }
        ));

        std::fs::remove_file(dir.join("BriskaBlast.x86_64")).unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::ExecutableMissing { .. }
        ));
    }

    /// A manifest entry with a traversal path must be rejected (reported as a
    /// failure), never followed outside the install dir.
    #[tokio::test]
    async fn verify_rejects_path_traversal_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("BriskaBlast.x86_64"), b"bin").unwrap();
        let installed = InstalledManifest {
            version: "0.17.0".into(),
            channel: "dev".into(),
            installed_at: "2026-06-25T00:00:00Z".into(),
            executable: "BriskaBlast.x86_64".into(),
        };
        std::fs::write(
            dir.join(MANIFEST_FILENAME),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();
        let mut files = BTreeMap::new();
        files.insert(
            "../escape.txt".to_string(),
            FileEntry {
                size: 1,
                sha256: "00".into(),
            },
        );
        std::fs::write(
            dir.join(FILES_MANIFEST_FILENAME),
            serde_json::to_vec(&FilesManifest { schema: 1, files }).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            verify_install(dir.to_path_buf()).await,
            VerifyOutcome::FilesMissing { count: 1, .. }
        ));
    }

    #[test]
    fn is_safe_relpath_rejects_escapes() {
        assert!(is_safe_relpath("a/b/c.dll"));
        assert!(is_safe_relpath("BriskaBlast.pck"));
        assert!(!is_safe_relpath("../escape"));
        assert!(!is_safe_relpath("a/../../b"));
        assert!(!is_safe_relpath("/abs/path"));
        assert!(!is_safe_relpath(""));
    }
}
