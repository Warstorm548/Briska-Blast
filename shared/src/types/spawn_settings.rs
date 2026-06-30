use serde::{Deserialize, Serialize};

/// Inclusive bounds on the BallSpliter spawn interval, in seconds. Single source of
/// truth shared by the host UI slider (hand-mirrored in C#) and the server's
/// authoritative `/host` validation, so the UX limit and the trust-boundary check
/// can never drift apart (same rationale as the win-condition bounds).
pub const SPLITTER_INTERVAL_MIN_SECS: u8 = 5;
pub const SPLITTER_INTERVAL_MAX_SECS: u8 = 60;
/// Default interval applied when a host never touches the setting.
pub const DEFAULT_SPLITTER_INTERVAL_SECS: u8 = 15;
/// Chain-splitting (a BallBT split ball can split again) defaults on.
pub const DEFAULT_CHAIN_SPLIT: bool = true;

/// Host-configured rules for the random-spawn elements (currently just the
/// BallSpliter). Travels as one field alongside `win_condition` in the host request
/// and is echoed to joiners; each client drives its own local spawner from these
/// values, so the cadence and chain-split rule are identical across the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnSettings {
    /// Seconds between BallSpliter spawns on each screen.
    pub splitter_interval_secs: u8,
    /// Whether BallBT split balls that hit another splitter split again.
    pub chain_split: bool,
}

impl SpawnSettings {
    /// The settings a host gets without opening advanced settings.
    pub fn default_settings() -> Self {
        SpawnSettings {
            splitter_interval_secs: DEFAULT_SPLITTER_INTERVAL_SECS,
            chain_split: DEFAULT_CHAIN_SPLIT,
        }
    }

    /// Validate against the allowed range. On failure returns `(min, max, requested)`
    /// for the splitter interval so the caller can surface a precise error. Enforced
    /// server-side (defense in depth) — a tampered client that bypasses the UI slider
    /// is refused here.
    pub fn validate(&self) -> Result<(), (u8, u8, u8)> {
        if self.splitter_interval_secs < SPLITTER_INTERVAL_MIN_SECS
            || self.splitter_interval_secs > SPLITTER_INTERVAL_MAX_SECS
        {
            Err((
                SPLITTER_INTERVAL_MIN_SECS,
                SPLITTER_INTERVAL_MAX_SECS,
                self.splitter_interval_secs,
            ))
        } else {
            Ok(())
        }
    }
}

impl Default for SpawnSettings {
    fn default() -> Self {
        SpawnSettings::default_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_flat_object() {
        let json = serde_json::to_string(&SpawnSettings {
            splitter_interval_secs: 15,
            chain_split: true,
        })
        .unwrap();
        assert_eq!(json, r#"{"splitter_interval_secs":15,"chain_split":true}"#);
    }

    #[test]
    fn default_is_fifteen_seconds_chain_on() {
        let d = SpawnSettings::default_settings();
        assert_eq!(d.splitter_interval_secs, DEFAULT_SPLITTER_INTERVAL_SECS);
        assert!(d.chain_split);
    }

    #[test]
    fn validate_accepts_in_range_including_bounds() {
        for secs in [SPLITTER_INTERVAL_MIN_SECS, 15, SPLITTER_INTERVAL_MAX_SECS] {
            assert!(SpawnSettings { splitter_interval_secs: secs, chain_split: true }
                .validate()
                .is_ok());
        }
    }

    #[test]
    fn validate_rejects_below_min() {
        let err = SpawnSettings { splitter_interval_secs: SPLITTER_INTERVAL_MIN_SECS - 1, chain_split: true }
            .validate()
            .unwrap_err();
        assert_eq!(
            err,
            (SPLITTER_INTERVAL_MIN_SECS, SPLITTER_INTERVAL_MAX_SECS, SPLITTER_INTERVAL_MIN_SECS - 1)
        );
    }

    #[test]
    fn validate_rejects_above_max() {
        let err = SpawnSettings { splitter_interval_secs: SPLITTER_INTERVAL_MAX_SECS + 1, chain_split: false }
            .validate()
            .unwrap_err();
        assert_eq!(
            err,
            (SPLITTER_INTERVAL_MIN_SECS, SPLITTER_INTERVAL_MAX_SECS, SPLITTER_INTERVAL_MAX_SECS + 1)
        );
    }
}
