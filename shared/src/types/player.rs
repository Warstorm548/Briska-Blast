use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerId(pub String);

impl PlayerId {
    /// Canonical 9-digit zero-padded format. Widened from 7 digits as part of
    /// the dev-flag rollout; pre-existing 7-digit ids in Redis are stored
    /// as plain strings and remain unique, so no migration is required.
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
}
