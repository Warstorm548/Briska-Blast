//! Co-located data layout: every launcher-managed file lives under
//! `<install_dir>/data/` so a single folder next to the binary holds
//! identity, saves, and anything else this slice adds later. Documented as
//! "for the time being" — see plan and roadmap; cross-platform `directories`
//! crate adoption is deferred until the schema stabilises.

use crate::channel::Channel;
use std::io;
use std::path::PathBuf;

/// Directory containing the currently-running launcher binary.
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

/// `<install_dir>/data/`. Created on first call.
pub fn data_dir() -> io::Result<PathBuf> {
    let dir = install_dir()?.join("data");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `<install_dir>/data/identity.json`.
pub fn identity_path() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("identity.json"))
}

/// `<install_dir>/data/saves/<channel>/`. Created on first call. Reserved for
/// the existing `Settings → Game Channel Management → Game Save` button row
/// once it gets a real implementation; exposing the path here is in scope so
/// future code doesn't have to re-derive the layout.
#[allow(dead_code)]
pub fn saves_dir(channel: Channel) -> io::Result<PathBuf> {
    let dir = data_dir()?.join("saves").join(channel.dir_name());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
