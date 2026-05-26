use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    Extended,
}

impl std::fmt::Display for GameMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameMode::Extended => write!(f, "extended"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_snake_case() {
        let json = serde_json::to_string(&GameMode::Extended).unwrap();
        assert_eq!(json, "\"extended\"");
    }

    #[test]
    fn deserializes_from_snake_case() {
        let mode: GameMode = serde_json::from_str("\"extended\"").unwrap();
        assert_eq!(mode, GameMode::Extended);
    }

    #[test]
    fn rejects_unknown_variant() {
        let result: Result<GameMode, _> = serde_json::from_str("\"banana\"");
        assert!(result.is_err());
    }

    #[test]
    fn display_matches_wire_format() {
        assert_eq!(format!("{}", GameMode::Extended), "extended");
    }
}
