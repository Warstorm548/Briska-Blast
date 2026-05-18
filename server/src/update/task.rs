use deadpool_redis::redis::AsyncCommands;
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::state::AppState;
use super::{docker, github, watchtower};

// Anything farther than this in the future is treated as a corrupted
// `update:scheduled_at` value on recovery. Picked at 30 days because no
// legitimate manual schedule should ever exceed a month.
const SCHEDULE_RECOVERY_MAX_FUTURE_SECS: i64 = 30 * 24 * 60 * 60;

pub enum UpdateCommand {
    ApplyNow,
    Schedule(i64),
    CancelSchedule,
    SettingsChanged,
}

pub async fn run(state: AppState, mut rx: mpsc::Receiver<UpdateCommand>) {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client for update task");
    let channel = env!("RELEASE_CHANNEL");

    // seed current version into Redis on startup
    if let Ok(mut conn) = state.redis.get().await {
        let _: () = conn
            .set("update:current_version", env!("CARGO_PKG_VERSION"))
            .await
            .inspect_err(|e| tracing::warn!("redis seed current_version failed: {e}"))
            .unwrap_or(());
    }

    // recover any pending manual schedule that survived a restart
    if let Ok(mut conn) = state.redis.get().await {
        let stored: String = conn
            .get("update:scheduled_at")
            .await
            .inspect_err(|e| tracing::warn!("redis get scheduled_at failed: {e}"))
            .unwrap_or_default();
        if let Ok(ts) = stored.parse::<i64>() {
            let now = chrono::Utc::now().timestamp();
            match classify_recovered_schedule(ts, now) {
                RecoveredSchedule::Corrupted => {
                    tracing::warn!(
                        "discarded scheduled_at={ts} on recovery: > {} days in the future (corrupted)",
                        SCHEDULE_RECOVERY_MAX_FUTURE_SECS / 86400
                    );
                    clear_schedule_conn(&mut conn).await;
                }
                RecoveredSchedule::Usable => {
                    let state2 = state.clone();
                    let client2 = client.clone();
                    tokio::spawn(async move {
                        wait_and_apply(ts, state2, client2).await;
                    });
                    tracing::info!("resumed pending scheduled update for ts={ts}");
                }
                RecoveredSchedule::AlreadyPassed => {
                    clear_schedule_conn(&mut conn).await;
                    tracing::warn!("discarded stale scheduled update (scheduled time already passed)");
                }
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
                    let enabled: String = conn
                        .get("update:auto_enabled")
                        .await
                        .inspect_err(|e| tracing::warn!("redis get auto_enabled failed: {e}"))
                        .unwrap_or_default();
                    if enabled == "true" {
                        do_check(&client, &state, channel).await;
                        maybe_apply(&client, &state, channel).await;
                    }
                    // refresh interval from Redis
                    let secs: String = conn
                        .get("update:check_interval_secs")
                        .await
                        .inspect_err(|e| tracing::warn!("redis get check_interval_secs failed: {e}"))
                        .unwrap_or_default();
                    poll_secs = secs.parse().unwrap_or(21600);
                }
                next_poll = Instant::now() + Duration::from_secs(poll_secs);
            }

            cmd = rx.recv() => {
                match cmd {
                    None => break,
                    Some(UpdateCommand::ApplyNow) => {
                        let _guard = state.update_apply_lock.lock().await;
                        // Re-check rollback lock *after* acquiring the apply
                        // lock — a rollback that completed while this command
                        // was queued must not be undone by a stale ApplyNow.
                        if rollback_locked(&state).await {
                            tracing::warn!("ApplyNow ignored: rollback_locked is set");
                            continue;
                        }
                        if let Err(e) = docker::pull_channel_image(channel).await {
                            tracing::warn!("ApplyNow pre-pull failed: {e}");
                        }
                        let ok = watchtower::trigger_update(
                            &client,
                            &state.config.watchtower_url,
                            &state.config.watchtower_token,
                        ).await;
                        if ok {
                            store_previous_version(&state).await;
                        }
                    }
                    Some(UpdateCommand::Schedule(ts)) => {
                        if let Ok(mut conn) = state.redis.get().await {
                            let _: () = conn
                                .set("update:scheduled_at", ts.to_string())
                                .await
                                .inspect_err(|e| tracing::warn!("redis set scheduled_at failed: {e}"))
                                .unwrap_or(());
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
                            let secs: String = conn
                                .get("update:check_interval_secs")
                                .await
                                .inspect_err(|e| tracing::warn!("redis get check_interval_secs failed: {e}"))
                                .unwrap_or_default();
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

    // `update:last_checked` is now written only on a successful round-trip
    // (Update / NoUpdate / NotModified). On Err we leave the previous value
    // alone — a network blip should not advance the timestamp to "now" and
    // mislead the dashboard into showing a fresh check that never happened.
    match github::check_for_update(client, channel, &state.redis).await {
        Ok(github::CheckOutcome::Update(tag)) => {
            if let Ok(mut conn) = state.redis.get().await {
                // Only reset found_at when this is a *new* version we haven't
                // seen before — otherwise repeated polls would keep resetting
                // the apply_interval window and auto-apply would never fire.
                let prev: String = conn
                    .get("update:available_version")
                    .await
                    .inspect_err(|e| tracing::warn!("redis get available_version failed: {e}"))
                    .unwrap_or_default();
                let _: () = conn
                    .set("update:available_version", &tag)
                    .await
                    .inspect_err(|e| tracing::warn!("redis set available_version failed: {e}"))
                    .unwrap_or(());
                if should_reset_found_at(&prev, &tag) {
                    let _: () = conn
                        .set("update:found_at", now.to_string())
                        .await
                        .inspect_err(|e| tracing::warn!("redis set found_at failed: {e}"))
                        .unwrap_or(());
                }
                set_last_checked(&mut conn, now).await;
            }
            tracing::info!("update available: {tag}");
        }
        Ok(github::CheckOutcome::NoUpdate) => {
            if let Ok(mut conn) = state.redis.get().await {
                let _: () = conn
                    .del("update:available_version")
                    .await
                    .inspect_err(|e| tracing::warn!("redis del available_version failed: {e}"))
                    .unwrap_or(());
                let _: () = conn
                    .del("update:found_at")
                    .await
                    .inspect_err(|e| tracing::warn!("redis del found_at failed: {e}"))
                    .unwrap_or(());
                set_last_checked(&mut conn, now).await;
            }
            tracing::debug!("no update available");
        }
        Ok(github::CheckOutcome::NotModified) => {
            // GitHub confirmed nothing has changed — preserve cached state but
            // still advance the last-checked timestamp; the check itself
            // succeeded.
            if let Ok(mut conn) = state.redis.get().await {
                set_last_checked(&mut conn, now).await;
            }
            tracing::debug!("github 304 Not Modified — preserving cached state");
        }
        Err(e) => {
            // Network/parse failure — preserve any previously known state so
            // a transient blip doesn't make an existing update "disappear",
            // and leave last_checked untouched.
            tracing::warn!("update check failed (state preserved): {e}");
        }
    }
}

async fn set_last_checked(conn: &mut deadpool_redis::Connection, now: i64) {
    let _: () = conn
        .set("update:last_checked", now.to_string())
        .await
        .inspect_err(|e| tracing::warn!("redis set last_checked failed: {e}"))
        .unwrap_or(());
}

async fn maybe_apply(client: &Client, state: &AppState, channel: &str) {
    // Acquire the apply lock FIRST, then re-read state. Any rollback or
    // settings change that completed while we were pending must be visible
    // to our "should we apply?" decision — otherwise a concurrent rollback
    // can be silently re-overwritten by a stale auto-apply (see v0.4.2).
    let _guard = state.update_apply_lock.lock().await;

    let mut conn = match state.redis.get().await {
        Ok(c) => c,
        Err(_) => return,
    };

    if !should_apply_after_lock(&mut conn).await {
        return;
    }

    let found_at: String = conn
        .get("update:found_at")
        .await
        .inspect_err(|e| tracing::warn!("redis get found_at failed: {e}"))
        .unwrap_or_default();
    let apply_interval: String = conn
        .get("update:apply_interval_secs")
        .await
        .inspect_err(|e| tracing::warn!("redis get apply_interval_secs failed: {e}"))
        .unwrap_or_default();

    let ready = if apply_interval.is_empty() || apply_interval == "0" {
        true
    } else {
        let interval_secs: i64 = apply_interval.parse().unwrap_or(0);
        let found: i64 = found_at.parse().unwrap_or(0);
        chrono::Utc::now().timestamp() >= found + interval_secs
    };

    if ready {
        drop(conn);
        if let Err(e) = docker::pull_channel_image(channel).await {
            tracing::warn!("auto-apply pre-pull failed: {e}");
        }
        let ok = watchtower::trigger_update(
            client,
            &state.config.watchtower_url,
            &state.config.watchtower_token,
        ).await;
        if ok {
            store_previous_version(state).await;
        }
    }
}

/// Re-read authoritative apply state from Redis *after* the apply lock has
/// been acquired, and decide whether the auto-apply path should proceed.
///
/// All four bail-out conditions are checked together so rollback (which sets
/// `rollback_locked` + `auto_enabled=false`) wins decisively over any
/// auto-apply that was pending when the rollback completed.
async fn should_apply_after_lock(conn: &mut deadpool_redis::Connection) -> bool {
    let auto_enabled: String = conn
        .get("update:auto_enabled")
        .await
        .inspect_err(|e| tracing::warn!("redis get auto_enabled failed: {e}"))
        .unwrap_or_default();
    let rollback_locked: String = conn
        .get("update:rollback_locked")
        .await
        .inspect_err(|e| tracing::warn!("redis get rollback_locked failed: {e}"))
        .unwrap_or_default();
    let scheduled: String = conn
        .get("update:scheduled_at")
        .await
        .inspect_err(|e| tracing::warn!("redis get scheduled_at failed: {e}"))
        .unwrap_or_default();
    let available: String = conn
        .get("update:available_version")
        .await
        .inspect_err(|e| tracing::warn!("redis get available_version failed: {e}"))
        .unwrap_or_default();
    decide_should_apply(&auto_enabled, &rollback_locked, &scheduled, &available)
}

/// Pure decision predicate for `should_apply_after_lock` — extracted so it
/// can be unit-tested without standing up a Redis instance.
fn decide_should_apply(
    auto_enabled: &str,
    rollback_locked: &str,
    scheduled_at: &str,
    available_version: &str,
) -> bool {
    auto_enabled == "true"
        && rollback_locked != "true"
        && scheduled_at.is_empty()
        && !available_version.is_empty()
}

/// Read `update:rollback_locked` and return true iff it is set to "true".
/// Used by the `ApplyNow` command handler to bail out on a stale command
/// that was queued before a rollback completed.
async fn rollback_locked(state: &AppState) -> bool {
    let Ok(mut conn) = state.redis.get().await else { return false };
    let v: String = conn
        .get("update:rollback_locked")
        .await
        .inspect_err(|e| tracing::warn!("redis get rollback_locked failed: {e}"))
        .unwrap_or_default();
    v == "true"
}

async fn wait_and_apply(ts: i64, state: AppState, client: Client) {
    let now = chrono::Utc::now().timestamp();
    let delay = (ts - now).max(0) as u64;
    tokio::time::sleep(Duration::from_secs(delay)).await;

    let channel = env!("RELEASE_CHANNEL");

    // Acquire the apply lock FIRST, then re-validate the schedule and check
    // rollback state. A rollback that completed during our sleep must win
    // over this stale schedule (see v0.4.2 race-condition fix).
    let _guard = state.update_apply_lock.lock().await;

    let stored_ts: Option<i64> = match state.redis.get().await {
        Ok(mut conn) => {
            let raw: String = conn
                .get("update:scheduled_at")
                .await
                .inspect_err(|e| tracing::warn!("redis get scheduled_at failed: {e}"))
                .unwrap_or_default();
            raw.parse::<i64>().ok()
        }
        Err(_) => None,
    };
    if !wait_and_apply_should_proceed(ts, stored_ts) {
        return;
    }
    if rollback_locked(&state).await {
        tracing::warn!("scheduled apply ignored: rollback_locked is set");
        return;
    }

    if let Err(e) = docker::pull_channel_image(channel).await {
        tracing::warn!("scheduled apply pre-pull failed: {e}");
    }
    let ok = watchtower::trigger_update(
        &client,
        &state.config.watchtower_url,
        &state.config.watchtower_token,
    ).await;
    // Only mutate Redis state once Watchtower has actually accepted the
    // trigger — a failed apply must not silently drop the schedule or the
    // rollback target.
    if ok {
        if let Ok(mut conn) = state.redis.get().await {
            store_previous_version_conn(&mut conn).await;
            clear_schedule_conn(&mut conn).await;
        }
    }
}

async fn store_previous_version(state: &AppState) {
    if let Ok(mut conn) = state.redis.get().await {
        store_previous_version_conn(&mut conn).await;
    }
}

async fn store_previous_version_conn(conn: &mut deadpool_redis::Connection) {
    let current = env!("CARGO_PKG_VERSION");
    let _: () = conn
        .set("update:previous_version", current)
        .await
        .inspect_err(|e| tracing::warn!("redis set previous_version failed: {e}"))
        .unwrap_or(());
}

async fn clear_schedule(state: &AppState) {
    if let Ok(mut conn) = state.redis.get().await {
        clear_schedule_conn(&mut conn).await;
    }
}

async fn clear_schedule_conn(conn: &mut deadpool_redis::Connection) {
    let _: () = conn
        .del("update:scheduled_at")
        .await
        .inspect_err(|e| tracing::warn!("redis del scheduled_at failed: {e}"))
        .unwrap_or(());
    let _: () = conn
        .del("update:scheduled_version")
        .await
        .inspect_err(|e| tracing::warn!("redis del scheduled_version failed: {e}"))
        .unwrap_or(());
}

/// Decide whether a `do_check` cycle should reset `update:found_at` for a
/// newly-observed tag, given the tag that was previously cached.
///
/// Returns true iff the tag is genuinely new — repeated polls returning the
/// same tag must NOT reset found_at, otherwise the apply_interval window
/// would keep sliding forward and auto-apply would never fire.
fn should_reset_found_at(prev_tag: &str, new_tag: &str) -> bool {
    prev_tag != new_tag
}

/// Decide whether a previously-spawned `wait_and_apply` future should still
/// proceed with its update, given the `scheduled_at` value currently in
/// Redis. Returns true iff the stored timestamp still matches the one this
/// future was spawned for — any mismatch (cancel, reschedule, corruption)
/// means a newer task owns the update and this one must no-op.
fn wait_and_apply_should_proceed(spawned_for: i64, stored_in_redis: Option<i64>) -> bool {
    stored_in_redis == Some(spawned_for)
}

/// Decide whether a recovered `update:scheduled_at` timestamp on startup is
/// usable, expired, or corrupted (too far in the future).
#[derive(Debug, PartialEq, Eq)]
enum RecoveredSchedule {
    Usable,
    AlreadyPassed,
    Corrupted,
}

fn classify_recovered_schedule(scheduled_at: i64, now: i64) -> RecoveredSchedule {
    if scheduled_at > now + SCHEDULE_RECOVERY_MAX_FUTURE_SECS {
        RecoveredSchedule::Corrupted
    } else if scheduled_at <= now {
        RecoveredSchedule::AlreadyPassed
    } else {
        RecoveredSchedule::Usable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_at_only_resets_on_new_tag() {
        // First poll: nothing was cached.
        assert!(should_reset_found_at("", "v0.5.0"));
        // Same tag observed again — must not reset (regression guard for
        // the apply-interval-never-fires bug).
        assert!(!should_reset_found_at("v0.5.0", "v0.5.0"));
        // Truly new tag arrives — reset.
        assert!(should_reset_found_at("v0.5.0", "v0.5.1"));
    }

    #[test]
    fn wait_and_apply_proceeds_only_on_exact_match() {
        // Stored ts matches what we were spawned for — go.
        assert!(wait_and_apply_should_proceed(1_700_000_000, Some(1_700_000_000)));
        // Cancelled while we slept — the key was deleted.
        assert!(!wait_and_apply_should_proceed(1_700_000_000, None));
        // Rescheduled while we slept — a different ts is now stored.
        assert!(!wait_and_apply_should_proceed(1_700_000_000, Some(1_700_003_600)));
        // Even a 1-second drift is a cancel-then-reschedule — must not proceed.
        assert!(!wait_and_apply_should_proceed(1_700_000_000, Some(1_700_000_001)));
    }

    #[test]
    fn decide_should_apply_requires_auto_enabled() {
        // Happy path: auto on, no rollback, no schedule, version available.
        assert!(decide_should_apply("true", "", "", "v0.5.0"));

        // Auto disabled (the rollback handler sets this to "false") — never apply.
        assert!(!decide_should_apply("false", "", "", "v0.5.0"));
        assert!(!decide_should_apply("", "", "", "v0.5.0"));
    }

    #[test]
    fn decide_should_apply_blocks_on_rollback_lock() {
        // Regression guard for the v0.4.2 race: a rollback that completed
        // while auto-apply was pending must keep auto-apply from firing
        // even if auto_enabled hadn't yet been re-read as "false".
        assert!(!decide_should_apply("true", "true", "", "v0.5.0"));
    }

    #[test]
    fn decide_should_apply_defers_to_pending_schedule() {
        // wait_and_apply owns the update when a schedule is pending.
        assert!(!decide_should_apply("true", "", "1700000000", "v0.5.0"));
    }

    #[test]
    fn decide_should_apply_requires_available_version() {
        // No update was found — nothing to do.
        assert!(!decide_should_apply("true", "", "", ""));
    }

    #[test]
    fn recovered_schedule_classification() {
        let now = 1_700_000_000_i64;
        let day = 86_400_i64;

        assert_eq!(classify_recovered_schedule(now - day, now), RecoveredSchedule::AlreadyPassed);
        assert_eq!(classify_recovered_schedule(now, now), RecoveredSchedule::AlreadyPassed);
        assert_eq!(classify_recovered_schedule(now + day, now), RecoveredSchedule::Usable);
        assert_eq!(classify_recovered_schedule(now + 29 * day, now), RecoveredSchedule::Usable);
        // 31 days out — definitely corruption (datetime-local UI can't reach this).
        assert_eq!(classify_recovered_schedule(now + 31 * day, now), RecoveredSchedule::Corrupted);
        // Year 2099 timestamp — absurdly corrupted.
        assert_eq!(classify_recovered_schedule(4_070_908_800, now), RecoveredSchedule::Corrupted);
    }
}
