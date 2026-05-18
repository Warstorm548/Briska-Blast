use bollard::{
    Docker,
    image::{CreateImageOptions, TagImageOptions},
};
use futures_util::TryStreamExt;

const IMAGE_REPO: &str = "ghcr.io/warstorm548/briska-blast";

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

    while let Some(_) = stream.try_next().await.map_err(|e| format!("pull failed: {e}"))? {}

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
