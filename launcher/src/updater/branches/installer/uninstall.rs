//! Tear down a channel's installation, optionally preserving player saves.

use chrono::Utc;
use std::path::PathBuf;

/// Subdirectory of the install_root (the parent of the install_dir) where
/// saves are moved when the user picks "Keep saves" during uninstall. Per
/// the Stage 7 design: backups are timestamped so a re-install / re-uninstall
/// cycle never overwrites prior saves.
pub const SAVES_BACKUP_DIRNAME: &str = ".briska-saves-backup";

/// Tear down a channel's installation. `install_dir` is the resolved
/// `<install_root>/<channel.dir_name()>/` recorded in identity.json.
///
/// `keep_saves` honours foundation §2's "Keep player data for future
/// reinstall?" prompt — when true, `<install_dir>/saves/` is moved to a
/// **timestamped** sibling under `<install_root>/.briska-saves-backup/
/// <channel.dir_name()>/<rfc3339>/` before the install dir is removed so
/// a subsequent reinstall + re-uninstall cycle never overwrites a prior
/// backup. When false the saves go with the install.
pub async fn uninstall_install(
    install_dir: PathBuf,
    channel_dir_name: &'static str,
    keep_saves: bool,
) -> Result<(), String> {
    if !install_dir.exists() {
        // Nothing on disk — treat as success so the caller can still
        // clear identity.json. The user did ask for "uninstalled", after all.
        return Ok(());
    }

    // Defence-in-depth: the launcher computes install_dir as
    // <user-picked install_root>/<channel.dir_name()>/, but
    // identity.json could be hand-edited to point install_location
    // somewhere else (e.g. /home/user, /etc). Before `remove_dir_all`
    // we canonicalize and require the final path component to match
    // the channel_dir_name passed in. Anything else returns Err — the
    // launcher will surface the message on the uninstall prompt
    // rather than wiping an unrelated directory.
    let canonical = tokio::fs::canonicalize(&install_dir).await.map_err(|e| {
        format!(
            "canonicalize install_dir {}: {e}",
            install_dir.display()
        )
    })?;
    let last = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            format!(
                "install_dir {} has no terminal component",
                canonical.display()
            )
        })?;
    if last != channel_dir_name {
        return Err(format!(
            "install_dir {} does not end in expected channel name {:?} — refusing to remove",
            canonical.display(),
            channel_dir_name
        ));
    }

    if keep_saves {
        let saves = install_dir.join("saves");
        if saves.exists() {
            let install_root = install_dir.parent().ok_or_else(|| {
                format!(
                    "install_dir {} has no parent — cannot place saves backup",
                    install_dir.display()
                )
            })?;
            // Millisecond precision avoids a collision when two
            // uninstalls land in the same calendar second (e.g. test
            // harnesses, rapid retries). The `%.3f` chrono token writes
            // `.NNN` with a literal dot — filesystem-safe on Linux,
            // macOS, and Windows (no `:`).
            let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
            let backup_dir = install_root
                .join(SAVES_BACKUP_DIRNAME)
                .join(channel_dir_name)
                .join(stamp);
            if let Some(parent) = backup_dir.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("create saves backup parent: {e}"))?;
            }
            tokio::fs::rename(&saves, &backup_dir).await.map_err(|e| {
                format!(
                    "move {} → {}: {e}",
                    saves.display(),
                    backup_dir.display()
                )
            })?;
            tracing::info!(
                from = %saves.display(),
                to = %backup_dir.display(),
                "saves backed up before uninstall"
            );
        }
    }

    tokio::fs::remove_dir_all(&install_dir)
        .await
        .map_err(|e| format!("remove install dir {}: {e}", install_dir.display()))?;
    Ok(())
}
