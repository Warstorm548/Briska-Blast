use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerId(pub String);

impl PlayerId {
    /// Zero-padded canonical id, **minimum width 9**. Widened from 7 digits
    /// as part of the dev-flag rollout; pre-existing 7-digit ids in Redis
    /// are stored as plain strings and remain unique, so no migration is
    /// required. Counters at or above 1_000_000_000 produce more than 9
    /// digits — the format is a minimum width, not an exact width, so the
    /// id space scales past 1B without silent truncation.
    pub fn from_counter(n: u64) -> Self {
        Self(format!("{n:09}"))
    }
}

impl std::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerId;

    #[test]
    fn from_counter_is_nine_digits_zero_padded() {
        assert_eq!(PlayerId::from_counter(42).to_string(), "000000042");
        assert_eq!(PlayerId::from_counter(1).to_string(), "000000001");
        assert_eq!(PlayerId::from_counter(123_456_789).to_string(), "123456789");
    }

    #[test]
    fn from_counter_widens_past_nine_digits_at_one_billion() {
        // Documents the "minimum width" contract: at the boundary the id
        // stays exactly nine digits, immediately past it the id grows
        // rather than silently truncating. Locks the format choice.
        assert_eq!(PlayerId::from_counter(999_999_999).to_string(), "999999999");
        assert_eq!(
            PlayerId::from_counter(1_000_000_000).to_string(),
            "1000000000"
        );
    }
}
