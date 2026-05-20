use crate::error::AppError;
use shared::types::gamemode::GameMode;

pub fn bounds_for(mode: GameMode) -> (u8, u8) {
    match mode {
        GameMode::Extended => (2, 4),
    }
}

pub fn validate_player_count(mode: GameMode, requested: u8) -> Result<(), AppError> {
    let (min, max) = bounds_for(mode);
    if requested < min || requested > max {
        return Err(AppError::InvalidPlayerCount { min, max, requested });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_bounds_are_2_to_4() {
        assert_eq!(bounds_for(GameMode::Extended), (2, 4));
    }

    #[test]
    fn all_bounds_are_non_empty() {
        for mode in [GameMode::Extended] {
            let (min, max) = bounds_for(mode);
            assert!(min <= max, "{mode:?}: min ({min}) must be <= max ({max})");
            assert!(min >= 1, "{mode:?}: min ({min}) must be at least 1");
        }
    }

    #[test]
    fn validate_accepts_in_range() {
        assert!(validate_player_count(GameMode::Extended, 2).is_ok());
        assert!(validate_player_count(GameMode::Extended, 3).is_ok());
        assert!(validate_player_count(GameMode::Extended, 4).is_ok());
    }

    #[test]
    fn validate_rejects_below_min() {
        let err = validate_player_count(GameMode::Extended, 1).unwrap_err();
        match err {
            AppError::InvalidPlayerCount { min, max, requested } => {
                assert_eq!((min, max, requested), (2, 4, 1));
            }
            other => panic!("expected InvalidPlayerCount, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_above_max() {
        let err = validate_player_count(GameMode::Extended, 5).unwrap_err();
        match err {
            AppError::InvalidPlayerCount { min, max, requested } => {
                assert_eq!((min, max, requested), (2, 4, 5));
            }
            other => panic!("expected InvalidPlayerCount, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_zero() {
        assert!(validate_player_count(GameMode::Extended, 0).is_err());
    }
}
