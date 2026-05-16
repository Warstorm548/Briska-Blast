use std::{net::IpAddr, num::NonZeroU32, sync::Arc, time::Duration};

use deadpool_redis::Pool;
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};

use crate::config::Config;

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
    pub rl_host: Arc<KeyedLimiter>,
    pub rl_join: Arc<KeyedLimiter>,
    pub rl_session: Arc<KeyedLimiter>,
    pub rl_admin_login: Arc<KeyedLimiter>,
}

impl AppState {
    pub fn new(redis: Pool, config: Config) -> Self {
        Self {
            redis,
            config: Arc::new(config),
            rl_register: make_limiter(5),
            rl_host: make_limiter(10),
            rl_join: make_limiter(20),
            rl_session: make_limiter(60),
            rl_admin_login: make_login_limiter(),
        }
    }
}
