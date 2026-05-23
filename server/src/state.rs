use std::{net::IpAddr, num::NonZeroU32, sync::Arc, time::Duration};

use deadpool_redis::Pool;
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use tokio::sync::{mpsc, Mutex};

use crate::config::Config;
use crate::signaling::SignalHub;
use crate::update::UpdateCommand;

pub type KeyedLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

fn make_limiter(per_minute: u32) -> Arc<KeyedLimiter> {
    let quota = Quota::per_minute(NonZeroU32::new(per_minute).unwrap());
    Arc::new(RateLimiter::keyed(quota))
}

fn make_login_limiter() -> Arc<KeyedLimiter> {
    // 5 attempts per 15 minutes: burst of 5, refill 1 token per 3 minutes
    let quota = Quota::with_period(Duration::from_secs(180))
        .unwrap()
        .allow_burst(NonZeroU32::new(5).unwrap());
    Arc::new(RateLimiter::keyed(quota))
}

#[derive(Clone)]
pub struct AppState {
    pub redis: Pool,
    pub config: Arc<Config>,
    pub rl_register: Arc<KeyedLimiter>,
    pub rl_me_username: Arc<KeyedLimiter>,
    pub rl_host: Arc<KeyedLimiter>,
    pub rl_join: Arc<KeyedLimiter>,
    pub rl_session: Arc<KeyedLimiter>,
    pub rl_admin_login: Arc<KeyedLimiter>,
    pub update_tx: Arc<mpsc::Sender<UpdateCommand>>,
    // Single-flight guard around the local Docker `:channel` ref AND
    // `watchtower::trigger_update`. All paths that mutate the channel image
    // tag — timer auto-apply, ApplyNow, scheduled apply, and admin rollback
    // — acquire this before doing their Docker ops, so the registry pull in
    // `pull_channel_image` cannot race the local retag in
    // `retag_for_rollback`. Also keeps the Redis state writes around the
    // Watchtower trigger from interleaving. Watchtower itself is idempotent.
    //
    // Single-writer assumption: this lock is sufficient only as long as this
    // Rust process is the only writer to the daemon's `:channel` ref. If a
    // second writer is ever introduced (manual `docker` CLI, another
    // launcher process), promote to a Redis-backed advisory lock
    // keyed on the image ref — the Docker Engine API has no native
    // tag/ref lock primitive (moby PR #37781 protects individual writes
    // from corruption, not from last-write-wins ordering).
    pub update_apply_lock: Arc<Mutex<()>>,
    /// Ephemeral per-process registry of WebSocket signaling rooms.
    /// Lives on AppState so handlers (/start, /ws/session/:code) receive
    /// it transparently via the same State<AppState> they already take.
    pub signal_hub: Arc<SignalHub>,
}

impl AppState {
    pub fn new(redis: Pool, config: Config, update_tx: Arc<mpsc::Sender<UpdateCommand>>) -> Self {
        Self {
            redis,
            config: Arc::new(config),
            rl_register: make_limiter(5),
            rl_me_username: make_limiter(5),
            rl_host: make_limiter(10),
            rl_join: make_limiter(20),
            rl_session: make_limiter(60),
            rl_admin_login: make_login_limiter(),
            update_tx,
            update_apply_lock: Arc::new(Mutex::new(())),
            signal_hub: Arc::new(SignalHub::default()),
        }
    }
}
