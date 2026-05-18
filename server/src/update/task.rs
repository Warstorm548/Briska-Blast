use deadpool_redis::redis::AsyncCommands;
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::state::AppState;
use super::{github, watchtower};

pub enum UpdateCommand {
    CheckNow,
    ApplyNow,
    Schedule(i64),
    CancelSchedule,
    SettingsChanged,
}

pub async fn run(state: AppState, mut rx: mpsc::Receiver<UpdateCommand>) {
    let client = Client::new();
    let channel = env!("RELEASE_CHANNEL");

    // seed current version into Redis on startup
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn
            .set("update:current_version", env!("CARGO_PKG_VERSION"))
            .await
            .unwrap_or(());
    }

    // recover any pending manual schedule that survived a restart
    if let Ok(mut conn) = state.redis.get().await {
        let stored: String = conn.get("update:scheduled_at").await.unwrap_or_default();
        if let Ok(ts) = stored.parse::<i64>() {
            if ts > chrono::Utc::now().timestamp() {
                let state2 = state.clone();
                let client2 = client.clone();
                tokio::spawn(async move {
                    wait_and_apply(ts, state2, client2).await;
                });
                tracing::info!("resumed pending scheduled update for ts={ts}");
            } else {
                // scheduled time already passed while server was down — clear stale keys
                let _: () = conn.del("update:scheduled_at").await.unwrap_or(());
                let _: () = conn.del("update:scheduled_version").await.unwrap_or(());
                tracing::warn!("discarded stale scheduled update (scheduled time already passed)");
            }
        }
    }

    // default poll interval: 6 hours; re-read from Redis each cycle
    let mut poll_secs: u64 = 21600;
    let mut next_poll = Instant::now() + Duration::from_secs(poll_secs);

    loop {
        let deadline = tokio::time::sleep_until(next_poll);
        tokio::select! {
            _ = deadline => {
                // auto-check cycle
                if let Ok(mut conn) = state.redis.get().await {
                    let enabled: String = conn.get("update:auto_enabled").await.unwrap_or_default();
                    let manual: String = conn.get("update:manual_override").await.unwrap_or_default();
                    if enabled == "true" && manual != "true" {
                        do_check(&client, &state, channel).await;
                        maybe_apply(&client, &state, channel).await;
                    }
                    // refresh interval from Redis
                    let secs: String = conn.get("update:check_interval_secs").await.unwrap_or_default();
                    poll_secs = secs.parse().unwrap_or(21600);
                }
                next_poll = Instant::now() + Duration::from_secs(poll_secs);
            }

            cmd = rx.recv() => {
                match cmd {
                    None => break,
                    Some(UpdateCommand::CheckNow) => {
                        do_check(&client, &state, channel).await;
                    }
                    Some(UpdateCommand::ApplyNow) => {
                        store_previous_version(&state).await;
                        watchtower::trigger_update(
                            &client,
                            &state.config.watchtower_url,
                            &state.config.watchtower_token,
                        ).await;
                    }
                    Some(UpdateCommand::Schedule(ts)) => {
                        if let Ok(mut conn) = state.redis.get().await {
                            let _: () = conn.set("update:scheduled_at", ts.to_string()).await.unwrap_or(());
                        }
                        // spawn a one-shot task for this schedule
                        let state2 = state.clone();
                        let client2 = client.clone();
                        tokio::spawn(async move {
                            wait_and_apply(ts, state2, client2).await;
                        });
                    }
                    Some(UpdateCommand::CancelSchedule) => {
                        clear_schedule(&state).await;
                    }
                    Some(UpdateCommand::SettingsChanged) => {
                        if let Ok(mut conn) = state.redis.get().await {
                            let secs: String = conn.get("update:check_interval_secs").await.unwrap_or_default();
                            poll_secs = secs.parse().unwrap_or(21600);
                        }
                        next_poll = Instant::now() + Duration::from_secs(poll_secs);
                    }
                }
            }
        }
    }
}

async fn do_check(client: &Client, state: &AppState, channel: &str) {
    let now = chrono::Utc::now().timestamp();
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn.set("update:last_checked", now.to_string()).await.unwrap_or(());
    }

    match github::check_for_update(client, channel).await {
        Some(tag) => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.set("update:available_version", &tag).await.unwrap_or(());
                let _: () = conn.set("update:found_at", now.to_string()).await.unwrap_or(());
            }
            tracing::info!("update available: {tag}");
        }
        None => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn.del("update:available_version").await.unwrap_or(());
                let _: () = conn.del("update:found_at").await.unwrap_or(());
            }
            tracing::debug!("no update available");
        }
    }
}

async fn maybe_apply(client: &Client, state: &AppState, _channel: &str) {
    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return,
    };

    let available: String = conn.get("update:available_version").await.unwrap_or_default();
    if available.is_empty() {
        return;
    }

    let found_at: String = conn.get("update:found_at").await.unwrap_or_default();
    let apply_interval: String = conn.get("update:apply_interval_secs").await.unwrap_or_default();

    let ready = if apply_interval.is_empty() || apply_interval == "0" {
        true
    } else {
        let interval_secs: i64 = apply_interval.parse().unwrap_or(0);
        let found: i64 = found_at.parse().unwrap_or(0);
        chrono::Utc::now().timestamp() >= found + interval_secs
    };

    if ready {
        store_previous_version_conn(&mut conn).await;
        drop(conn);
        watchtower::trigger_update(
            client,
            &state.config.watchtower_url,
            &state.config.watchtower_token,
        ).await;
    }
}

async fn wait_and_apply(ts: i64, state: AppState, client: Client) {
    let now = chrono::Utc::now().timestamp();
    let delay = (ts - now).max(0) as u64;
    tokio::time::sleep(Duration::from_secs(delay)).await;

    // confirm schedule wasn't cancelled while we waited
    if let Ok(mut conn) = state.redis.get().await {
        let stored: String = conn.get("update:scheduled_at").await.unwrap_or_default();
        if stored.parse::<i64>().unwrap_or(0) != ts {
            return;
        }
        store_previous_version_conn(&mut conn).await;
        clear_schedule_conn(&mut conn).await;
    }

    watchtower::trigger_update(
        &client,
        &state.config.watchtower_url,
        &state.config.watchtower_token,
    ).await;
}

async fn store_previous_version(state: &AppState) {
    if let Ok(mut conn) = state.redis.get().await {
        store_previous_version_conn(&mut conn).await;
    }
}

async fn store_previous_version_conn(conn: &mut deadpool_redis::Connection) {
    let current = env!("CARGO_PKG_VERSION");
    let _: () = conn.set("update:previous_version", current).await.unwrap_or(());
}

async fn clear_schedule(state: &AppState) {
    if let Ok(mut conn) = state.redis.get().await {
        clear_schedule_conn(&mut conn).await;
    }
}

async fn clear_schedule_conn(conn: &mut deadpool_redis::Connection) {
    let _: () = conn.del("update:scheduled_at").await.unwrap_or(());
    let _: () = conn.del("update:scheduled_version").await.unwrap_or(());
}
