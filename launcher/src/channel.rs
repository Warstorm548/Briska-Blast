//! Channel taxonomy. Each variant connects to a different deployed server.
//! See docs/launcher/launcher-foundation.md §3.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Ea,
    Dev,
}

impl Channel {
    /// Server hostname this channel reaches.
    /// MUST match the constants in `client/BriskaBlast.csproj`'s
    /// GenerateBuildConfig target so launcher and client agree.
    #[allow(dead_code)] // consumed by the v1.x reach-out / network slice
    pub const fn host(self) -> &'static str {
        match self {
            Channel::Stable => "briska.phoenixwired.com",
            Channel::Ea => "briskaea.phoenixwired.com",
            Channel::Dev => "briskadev.phoenixwired.com",
        }
    }

    #[allow(dead_code)] // consumed by the v1.x reach-out / network slice
    pub const fn all() -> [Channel; 3] {
        [Self::Stable, Self::Ea, Self::Dev]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Channel::Stable => "Stable",
            Channel::Ea => "EA",
            Channel::Dev => "Dev",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
