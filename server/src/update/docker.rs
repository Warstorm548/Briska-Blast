use bollard::{
    Docker,
    image::{CreateImageOptions, TagImageOptions},
};
use futures_util::TryStreamExt;

// Hardcoded by design — see ServerChangeLog "Security Notes" for v0.4.1.
// Keeping the registry in the compiled binary means a runtime environment
// compromise (.env tampering, container env injection) cannot redirect the
// update / rollback path at a malicious registry.
const IMAGE_REPO: &str = "ghcr.io/warstorm548/briska-blast";

/// Pull the newest image for the given channel tag (e.g. `:stable`) into the
/// local Docker daemon. Used by the auto-apply path to stage the new image
/// before triggering Watchtower.
///
/// Why this exists: Watchtower runs with `WATCHTOWER_NO_PULL=true` (see
/// `docker-compose.yml`) so that the rollback flow's local retag is not
/// overwritten by a registry pull. The trade-off is that Watchtower no
/// longer pulls on its own, so the apply path must stage the image itself.
pub async fn pull_channel_image(channel: &str) -> Result<(), String> {
    let docker = Docker::connect_with_socket_defaults()
        .map_err(|e| format!("docker connect: {e}"))?;

    let from_image = format!("{}:{}", IMAGE_REPO, channel);

    let mut stream = docker.create_image(
        Some(CreateImageOptions {
            from_image: from_image.clone(),
            ..Default::default()
        }),
        None,
        None,
    );

    while stream.try_next().await.map_err(|e| format!("pull failed: {e}"))?.is_some() {}

    tracing::info!("pre-pulled {}", from_image);
    Ok(())
}

/// Pull a pinned versioned tag (e.g. `:v0.3.0`) and locally retag it as the
/// channel tag (e.g. `:stable`), so Watchtower's next restart picks up the
/// older binary as the "current" image.
///
/// **Call path**: this function is invoked synchronously from the
/// `POST /admin/update/rollback` handler in `admin::dashboard::rollback_update`,
/// not through the `UpdateCommand` channel. Rollback is admin-initiated,
/// blocks the HTTP response on success/failure, and must not queue behind
/// other pending update commands — therefore no `UpdateCommand::Rollback`
/// variant exists.
///
/// **Required env**: Watchtower must run with `WATCHTOWER_NO_PULL=true`
/// (set in `docker-compose.yml`). Without that, Watchtower's poll/trigger
/// would pull the registry's newer channel image immediately after this
/// retag and silently undo the rollback.
pub async fn retag_for_rollback(versioned_tag: &str, channel: &str) -> Result<(), String> {
    let docker = Docker::connect_with_socket_defaults()
        .map_err(|e| format!("docker connect: {e}"))?;

    let from_image = format!("{}:{}", IMAGE_REPO, versioned_tag);

    let mut stream = docker.create_image(
        Some(CreateImageOptions {
            from_image: from_image.clone(),
            ..Default::default()
        }),
        None,
        None,
    );

    while stream.try_next().await.map_err(|e| format!("pull failed: {e}"))?.is_some() {}

    docker
        .tag_image(
            &from_image,
            Some(TagImageOptions {
                repo: IMAGE_REPO,
                tag: channel,
            }),
        )
        .await
        .map_err(|e| format!("retag failed: {e}"))?;

    tracing::info!("retagged {}:{} as {}:{}", IMAGE_REPO, versioned_tag, IMAGE_REPO, channel);
    Ok(())
}
