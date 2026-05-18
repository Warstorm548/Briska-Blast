use reqwest::Client;

pub async fn trigger_update(client: &Client, url: &str, token: &str) -> bool {
    let result = client
        .post(format!("{}/v1/update", url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await;

    match result {
        Ok(r) => r.status().is_success(),
        Err(e) => {
            tracing::error!("watchtower trigger failed: {e}");
            false
        }
    }
}
