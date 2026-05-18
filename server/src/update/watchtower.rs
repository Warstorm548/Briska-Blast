use reqwest::Client;

pub async fn trigger_update(client: &Client, url: &str, token: &str) -> bool {
    let result = client
        .post(format!("{}/v1/update", url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await;

    match result {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                true
            } else {
                let body = r.text().await.unwrap_or_default();
                tracing::error!("watchtower returned {status}: {body}");
                false
            }
        }
        Err(e) => {
            tracing::error!("watchtower trigger failed: {e}");
            false
        }
    }
}
