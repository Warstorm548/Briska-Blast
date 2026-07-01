//! Archive extraction + executable resolution.
//!
//! Unpacks the downloaded `.tar.gz` (Linux/macOS) or `.zip` (Windows) into the
//! staging dir and resolves the game executable's path relative to that dir, so
//! `installed.json` can record it. All blocking I/O — the caller runs it inside
//! `spawn_blocking`.

use std::path::Path;

pub(super) fn extract_archive_blocking(
    archive: &Path,
    dest: &Path,
    name: &str,
) -> Result<String, String> {
    use std::fs::File;
    if name.ends_with(".tar.gz") {
        let f = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest)
            .map_err(|e| format!("tar unpack: {e}"))?;
        // Linux ships a bare ELF (BriskaBlast.x86_64) at the archive top level;
        // macOS ships a BriskaBlast.app bundle whose Mach-O lives a few levels
        // deep (Contents/MacOS/). Both arrive as .tar.gz, so the executable
        // resolution — not the extraction — is what differs by platform.
        #[cfg(target_os = "macos")]
        {
            find_app_executable(dest).ok_or_else(|| {
                "extracted archive does not contain a *.app/Contents/MacOS/ executable".to_string()
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            find_executable(dest, "BriskaBlast.x86_64")
                .ok_or_else(|| "extracted archive does not contain BriskaBlast.x86_64".to_string())
        }
    } else if name.ends_with(".zip") {
        let f = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let mut z = zip::ZipArchive::new(f).map_err(|e| format!("zip open: {e}"))?;
        z.extract(dest).map_err(|e| format!("zip extract: {e}"))?;
        find_executable(dest, "BriskaBlast.exe")
            .ok_or_else(|| "extracted archive does not contain BriskaBlast.exe".to_string())
    } else {
        Err(format!("unsupported archive extension: {name}"))
    }
}

/// Look for the game executable at the top level of `dir`, falling back to a
/// one-level-deep search in case the archive wraps everything in a single
/// directory (a common Godot export quirk).
fn find_executable(dir: &Path, name: &str) -> Option<String> {
    // is_file (not exists) so a directory that happens to share the executable's
    // name is never mistaken for the binary — matching `find_app_executable`.
    if dir.join(name).is_file() {
        return Some(name.to_string());
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join(name);
            if candidate.is_file() {
                return candidate
                    .strip_prefix(dir)
                    .ok()
                    .map(|p| p.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Locate the macOS game executable inside an extracted `.app` bundle. Finds the
/// single top-level `*.app`, then the Mach-O in its `Contents/MacOS/` (Godot
/// names it after the bundle, but we glob for the one file rather than hardcode
/// the name). Returns the path relative to `dir`, e.g.
/// `BriskaBlast.app/Contents/MacOS/BriskaBlast`, to store as the manifest
/// executable. Only called on macOS; the logic is OS-agnostic so the test below
/// exercises it on every platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn find_app_executable(dir: &Path) -> Option<String> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let app = entry.path();
        if app.is_dir() && app.extension().and_then(|e| e.to_str()) == Some("app") {
            let macos_dir = app.join("Contents").join("MacOS");
            for bin in std::fs::read_dir(&macos_dir).ok()?.flatten() {
                let p = bin.path();
                if p.is_file() {
                    return p
                        .strip_prefix(dir)
                        .ok()
                        .map(|r| r.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The in-bundle resolver returns the Mach-O path relative to the install
    /// dir, e.g. `BriskaBlast.app/Contents/MacOS/BriskaBlast`. OS-agnostic logic,
    /// so this runs on every platform's CI leg.
    #[test]
    fn find_app_executable_locates_in_bundle_macho() {
        let tmp = tempfile::tempdir().unwrap();
        let macos = tmp
            .path()
            .join("BriskaBlast.app")
            .join("Contents")
            .join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("BriskaBlast"), b"\x7fELF-or-macho").unwrap();

        let rel = find_app_executable(tmp.path()).expect("should find the in-bundle executable");
        assert_eq!(rel, "BriskaBlast.app/Contents/MacOS/BriskaBlast");
    }

    #[test]
    fn find_app_executable_none_without_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("BriskaBlast.x86_64"), b"elf").unwrap();
        assert!(find_app_executable(tmp.path()).is_none());
    }
}
