use shared::types::gamemode::GameMode;

pub fn bounds_for(mode: GameMode) -> (u8, u8) {
    match mode {
        GameMode::Extended => (2, 4),
    }
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
}
