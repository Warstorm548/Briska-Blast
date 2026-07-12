use serde::{Deserialize, Serialize};

/// Inclusive bounds on a `SetScore` target. Single source of truth shared by the
/// client's input cap (hand-mirrored in C#) and the server's authoritative `/host`
/// validation, so the UX limit and the trust-boundary check can never drift apart
/// (same rationale as `MAX_USERNAME_LEN`).
pub const SET_SCORE_MIN: u8 = 50;
pub const SET_SCORE_MAX: u8 = 200;
/// Default target applied when a host never touches the advanced setting.
pub const DEFAULT_TARGET: u8 = 100;

/// How a match decides a winner. The first (and currently only) variant is
/// `SetScore`: first player to reach `target` points ends the game. Internally
/// tagged on `kind` so the wire shape is a flat object — `{"kind":"set_score",
/// "target":100}` — which the hand-mirrored C# record round-trips both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WinCondition {
    SetScore { target: u8 },
}

impl WinCondition {
    /// The win condition a host gets without opening advanced settings.
    pub fn default_condition() -> Self {
        WinCondition::SetScore { target: DEFAULT_TARGET }
    }

    /// The point total that ends the match.
    pub fn target(&self) -> u8 {
        match self {
            WinCondition::SetScore { target } => *target,
        }
    }

    /// Validate the condition against its allowed range. On failure returns
    /// `(min, max, requested)` so the caller can surface a precise error. Enforced
    /// server-side too (defense in depth) — a tampered client that bypasses the UI
    /// cap is refused here.
    pub fn validate(&self) -> Result<(), (u8, u8, u8)> {
        match self {
            WinCondition::SetScore { target } => {
                if *target < SET_SCORE_MIN || *target > SET_SCORE_MAX {
                    Err((SET_SCORE_MIN, SET_SCORE_MAX, *target))
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl Default for WinCondition {
    fn default() -> Self {
        WinCondition::default_condition()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_set_score_to_tagged_object() {
        let json = serde_json::to_string(&WinCondition::SetScore { target: 100 }).unwrap();
        assert_eq!(json, r#"{"kind":"set_score","target":100}"#);
    }

    #[test]
    fn deserializes_from_tagged_object() {
        let wc: WinCondition =
            serde_json::from_str(r#"{"kind":"set_score","target":75}"#).unwrap();
        assert_eq!(wc, WinCondition::SetScore { target: 75 });
    }

    #[test]
    fn rejects_unknown_kind() {
        let result: Result<WinCondition, _> =
            serde_json::from_str(r#"{"kind":"banana","target":11}"#);
        assert!(result.is_err());
    }

    #[test]
    fn default_is_set_score_hundred() {
        assert_eq!(
            WinCondition::default_condition(),
            WinCondition::SetScore { target: DEFAULT_TARGET }
        );
        assert_eq!(WinCondition::default_condition().target(), 100);
    }

    #[test]
    fn validate_accepts_in_range_including_bounds() {
        assert!(WinCondition::SetScore { target: SET_SCORE_MIN }.validate().is_ok());
        assert!(WinCondition::SetScore { target: SET_SCORE_MAX }.validate().is_ok());
        assert!(WinCondition::SetScore { target: 100 }.validate().is_ok());
    }

    #[test]
    fn validate_rejects_below_min() {
        let err = WinCondition::SetScore { target: SET_SCORE_MIN - 1 }
            .validate()
            .unwrap_err();
        assert_eq!(err, (SET_SCORE_MIN, SET_SCORE_MAX, SET_SCORE_MIN - 1));
    }

    #[test]
    fn validate_rejects_above_max() {
        let err = WinCondition::SetScore { target: SET_SCORE_MAX + 1 }
            .validate()
            .unwrap_err();
        assert_eq!(err, (SET_SCORE_MIN, SET_SCORE_MAX, SET_SCORE_MAX + 1));
    }
}
