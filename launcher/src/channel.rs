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

    /// Lowercase identifier used on disk (`data/saves/<dir_name>/`) and in
    /// serde wire format. Matches the `rename_all = "lowercase"` above.
    pub const fn dir_name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Ea => "ea",
            Channel::Dev => "dev",
        }
    }

    /// Identity baked into the game's .NET runtime-cache folder name
    /// (`%LOCALAPPDATA%\data_<cache_basename>_windows_x86_64`). Each channel
    /// gets a distinct value so their runtime caches never collide once more
    /// than one channel is installed. **MUST stay in lockstep with the client
    /// export's per-channel project identity** — the game build sets the
    /// matching name at export time, and `paths::runtime_cache_dir` /
    /// `clear_runtime_cache` (the Reset Runtime Cache button) derive the folder
    /// from this. Stable keeps the bare `BriskaBlast` (unchanged on disk).
    pub const fn cache_basename(self) -> &'static str {
        match self {
            Channel::Stable => "BriskaBlast",
            Channel::Ea => "BriskaBlastEA",
            Channel::Dev => "BriskaBlastDev",
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;

    /// The runtime-cache basenames are the contract with the client export's
    /// per-channel project identity. If these change, the game build's export
    /// step must change in lockstep or the Reset Runtime Cache button will look
    /// for the wrong folder. Stable stays the bare name (unchanged on disk).
    #[test]
    fn cache_basenames_are_distinct_and_stable_is_unsuffixed() {
        assert_eq!(Channel::Stable.cache_basename(), "BriskaBlast");
        assert_eq!(Channel::Ea.cache_basename(), "BriskaBlastEA");
        assert_eq!(Channel::Dev.cache_basename(), "BriskaBlastDev");
        // All distinct → no cross-channel cache collision.
        let names = [
            Channel::Stable.cache_basename(),
            Channel::Ea.cache_basename(),
            Channel::Dev.cache_basename(),
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
