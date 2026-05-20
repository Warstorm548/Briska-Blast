use reqwest::Client;

pub async fn trigger_update(client: &Client, url: &str, token: &str) -> bool {
    // Normalise the base URL — a trailing slash on WATCHTOWER_URL would otherwise
    // produce `.../v1/update` with a double slash, which some reverse proxies
    // reject before the request ever reaches Watchtower.
    let endpoint = format!("{}/v1/update", url.trim_end_matches('/'));
    let result = client
        .post(endpoint)
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
