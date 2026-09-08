use serde::{Deserialize, Serialize};

/// Inclusive bounds on the loot-drop cadence, in seconds. Single source of truth
/// shared by the host UI slider (hand-mirrored in C#) and the server's authoritative
/// `/host` validation, so the UX limit and the trust-boundary check can never drift
/// apart (same rationale as `SpawnSettings`).
pub const DROP_INTERVAL_MIN_SECS: u8 = 5;
pub const DROP_INTERVAL_MAX_SECS: u8 = 60;
/// Cadence applied when a host never opens the Loot Table tab.
pub const DEFAULT_DROP_INTERVAL_SECS: u8 = 20;

/// Inclusive bounds on a single item's drop weight.
pub const WEIGHT_MIN: u8 = 1;
pub const WEIGHT_MAX: u8 = 100;

/// The subscribed total can never exceed this. See [`LootSettings::subscribed_total`]
/// for what "subscribed" means — it is the sum of DISTINCT weights, not of weights.
pub const WEIGHT_TOTAL_MAX: u16 = 100;

/// Inclusive bounds on how long one Full Barrier activation lasts.
pub const BARRIER_DURATION_MIN_SECS: u8 = 5;
pub const BARRIER_DURATION_MAX_SECS: u8 = 120;

pub const DEFAULT_BARRIER_ENABLED: bool = true;
pub const DEFAULT_BARRIER_WEIGHT: u8 = 50;
pub const DEFAULT_BARRIER_DURATION_SECS: u8 = 30;

/// How many items the loot table holds. Adding item #2 means bumping this, adding
/// its three fields below, and adding one entry to [`LootSettings::entries`] —
/// nothing else in this file changes.
pub const LOOT_ITEM_COUNT: usize = 1;

/// Host-configured loot-table rules. Travels as one field alongside `win_condition`
/// and `spawn_settings` in the host request and is echoed to joiners; each client
/// rolls its own local drops from these values, so the odds are identical across the
/// table even though the rolls are independent.
///
/// Flat per-item fields rather than a `Vec<LootEntry>` on purpose: a `Vec` that can be
/// empty hits the lua-cjson round-trip pitfall documented in the server's session
/// module (Redis re-encodes an empty `[]` as `{}` and the Rust deserialize then 500s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LootSettings {
    /// Seconds between loot rolls on each screen. Independent of the BallSpliter
    /// cadence in `SpawnSettings` — a roll can also come up empty (see
    /// [`LootSettings::nothing_rate`]), so this is the cadence of *attempts*.
    pub drop_interval_secs: u8,
    /// Whether the Full Barrier is in the table at all. A disabled item contributes
    /// nothing to the subscribed total and can never drop.
    pub barrier_enabled: bool,
    /// The Full Barrier's drop weight, 1–100.
    pub barrier_weight: u8,
    /// Seconds one Full Barrier activation adds to the shield timer.
    pub barrier_duration_secs: u8,
}

/// Which field failed validation, with the bounds that rejected it, so the server can
/// name the offending setting instead of a generic "invalid loot settings".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LootSettingsError {
    pub field: &'static str,
    pub min: u16,
    pub max: u16,
    pub requested: u16,
}

impl LootSettings {
    /// The settings a host gets without opening the Loot Table tab.
    pub fn default_settings() -> Self {
        LootSettings {
            drop_interval_secs: DEFAULT_DROP_INTERVAL_SECS,
            barrier_enabled: DEFAULT_BARRIER_ENABLED,
            barrier_weight: DEFAULT_BARRIER_WEIGHT,
            barrier_duration_secs: DEFAULT_BARRIER_DURATION_SECS,
        }
    }

    /// Every item's `(enabled, weight)` pair in a fixed order. The one place the
    /// item list is enumerated — the weighting maths below is written against this
    /// array and is already generic over N.
    pub fn entries(&self) -> [(bool, u8); LOOT_ITEM_COUNT] {
        [(self.barrier_enabled, self.barrier_weight)]
    }

    /// Sum of the **distinct** weight values among enabled items.
    ///
    /// Distinct, not summed per item: two items sharing a weight share one bucket
    /// and split it, so they add that weight to the total **once**. A lone item at
    /// 50 therefore drops on half of all rolls, and two items both at 50 drop on
    /// 25% of rolls each — still half the rolls between them, not all of them.
    ///
    /// The remainder up to 100 is the chance nothing drops.
    pub fn subscribed_total(&self) -> u16 {
        let entries = self.entries();
        let mut total: u16 = 0;
        for (i, &(enabled, weight)) in entries.iter().enumerate() {
            if !enabled {
                continue;
            }
            // Count a weight only the first time it appears among enabled items.
            if entries[..i]
                .iter()
                .any(|&(prev_on, prev_w)| prev_on && prev_w == weight)
            {
                continue;
            }
            total += weight as u16;
        }
        total
    }

    /// Each item's actual drop chance as a percentage of all rolls, in `entries`
    /// order. A disabled item is `0.0`. Items tied on a weight split it evenly, so
    /// these can be fractional even though the weights are integers.
    ///
    /// The values sum to [`subscribed_total`](Self::subscribed_total); whatever is
    /// left up to 100 is [`nothing_rate`](Self::nothing_rate).
    pub fn resolved_rates(&self) -> [f32; LOOT_ITEM_COUNT] {
        let entries = self.entries();
        let mut out = [0.0f32; LOOT_ITEM_COUNT];
        for (i, &(enabled, weight)) in entries.iter().enumerate() {
            if !enabled {
                continue;
            }
            let tied = entries
                .iter()
                .filter(|&&(on, w)| on && w == weight)
                .count();
            out[i] = weight as f32 / tied as f32;
        }
        out
    }

    /// The chance a roll produces nothing at all, as a percentage.
    pub fn nothing_rate(&self) -> f32 {
        (WEIGHT_TOTAL_MAX as f32 - self.subscribed_total() as f32).max(0.0)
    }

    /// Validate every field and the subscribed total. Enforced server-side as defense
    /// in depth — the host UI already caps each slider and shrinks the weight sliders
    /// to the remaining headroom, but a tampered client that bypasses the UI is
    /// refused here.
    pub fn validate(&self) -> Result<(), LootSettingsError> {
        fn range(
            field: &'static str,
            value: u8,
            min: u8,
            max: u8,
        ) -> Result<(), LootSettingsError> {
            if value < min || value > max {
                Err(LootSettingsError {
                    field,
                    min: min as u16,
                    max: max as u16,
                    requested: value as u16,
                })
            } else {
                Ok(())
            }
        }

        range(
            "drop_interval_secs",
            self.drop_interval_secs,
            DROP_INTERVAL_MIN_SECS,
            DROP_INTERVAL_MAX_SECS,
        )?;
        // Validated even when the item is disabled: the weight round-trips either
        // way, and accepting garbage in a disabled field would let it become live
        // the moment a host flips the toggle.
        range("barrier_weight", self.barrier_weight, WEIGHT_MIN, WEIGHT_MAX)?;
        range(
            "barrier_duration_secs",
            self.barrier_duration_secs,
            BARRIER_DURATION_MIN_SECS,
            BARRIER_DURATION_MAX_SECS,
        )?;

        let total = self.subscribed_total();
        if total > WEIGHT_TOTAL_MAX {
            return Err(LootSettingsError {
                field: "weight_total",
                min: 0,
                max: WEIGHT_TOTAL_MAX,
                requested: total,
            });
        }

        Ok(())
    }
}

impl Default for LootSettings {
    fn default() -> Self {
        LootSettings::default_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> LootSettings {
        LootSettings::default_settings()
    }

    #[test]
    fn serializes_to_flat_object() {
        let json = serde_json::to_string(&LootSettings {
            drop_interval_secs: 20,
            barrier_enabled: true,
            barrier_weight: 50,
            barrier_duration_secs: 30,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"drop_interval_secs":20,"barrier_enabled":true,"barrier_weight":50,"barrier_duration_secs":30}"#
        );
    }

    #[test]
    fn default_is_twenty_second_cadence_barrier_on_at_fifty_for_thirty() {
        let d = defaults();
        assert_eq!(d.drop_interval_secs, DEFAULT_DROP_INTERVAL_SECS);
        assert!(d.barrier_enabled);
        assert_eq!(d.barrier_weight, DEFAULT_BARRIER_WEIGHT);
        assert_eq!(d.barrier_duration_secs, DEFAULT_BARRIER_DURATION_SECS);
    }

    // --- the weighting rules -------------------------------------------------

    #[test]
    fn lone_item_weight_is_its_literal_drop_percentage() {
        let s = LootSettings { barrier_weight: 50, ..defaults() };
        assert_eq!(s.subscribed_total(), 50);
        assert_eq!(s.resolved_rates()[0], 50.0);
        assert_eq!(s.nothing_rate(), 50.0);
    }

    #[test]
    fn disabled_item_contributes_nothing_and_never_drops() {
        let s = LootSettings { barrier_enabled: false, barrier_weight: 90, ..defaults() };
        assert_eq!(s.subscribed_total(), 0);
        assert_eq!(s.resolved_rates()[0], 0.0);
        assert_eq!(s.nothing_rate(), 100.0);
    }

    #[test]
    fn rates_always_sum_to_the_subscribed_total() {
        for w in [WEIGHT_MIN, 10, 50, WEIGHT_MAX] {
            let s = LootSettings { barrier_weight: w, ..defaults() };
            let sum: f32 = s.resolved_rates().iter().sum();
            assert_eq!(sum, s.subscribed_total() as f32, "weight {w}");
            assert_eq!(sum + s.nothing_rate(), 100.0, "weight {w}");
        }
    }

    /// The tied-weight rule, exercised directly on the maths rather than through
    /// `LootSettings` — the struct only carries one item today, so this is what
    /// guards the behaviour until item #2 exists. Mirrors the algorithm in
    /// `subscribed_total` / `resolved_rates` over an arbitrary entry list.
    #[test]
    fn tied_weights_share_one_bucket_and_split_it() {
        fn total(entries: &[(bool, u8)]) -> u16 {
            let mut t = 0u16;
            for (i, &(on, w)) in entries.iter().enumerate() {
                if on && !entries[..i].iter().any(|&(p, pw)| p && pw == w) {
                    t += w as u16;
                }
            }
            t
        }
        fn rates(entries: &[(bool, u8)]) -> Vec<f32> {
            entries
                .iter()
                .map(|&(on, w)| {
                    if !on {
                        return 0.0;
                    }
                    let tied = entries.iter().filter(|&&(o, x)| o && x == w).count();
                    w as f32 / tied as f32
                })
                .collect()
        }

        // Two items both at 50: ONE 50 is subscribed, and they split it.
        let two = [(true, 50u8), (true, 50u8)];
        assert_eq!(total(&two), 50);
        assert_eq!(rates(&two), vec![25.0, 25.0]);

        // 10 / 50 / 50 -> distinct {10, 50} = 60; the pair splits the 50.
        let three = [(true, 10u8), (true, 50u8), (true, 50u8)];
        assert_eq!(total(&three), 60);
        assert_eq!(rates(&three), vec![10.0, 25.0, 25.0]);

        // 40 / 40 / 20 -> distinct {40, 20} = 60; all three end up equal at 20.
        let mixed = [(true, 40u8), (true, 40u8), (true, 20u8)];
        assert_eq!(total(&mixed), 60);
        assert_eq!(rates(&mixed), vec![20.0, 20.0, 20.0]);

        // A disabled item is not part of any bucket, so it does not dilute a tie.
        let with_off = [(true, 50u8), (false, 50u8)];
        assert_eq!(total(&with_off), 50);
        assert_eq!(rates(&with_off), vec![50.0, 0.0]);
    }

    // --- validation ----------------------------------------------------------

    #[test]
    fn validate_accepts_defaults_and_bounds() {
        assert!(defaults().validate().is_ok());
        for secs in [DROP_INTERVAL_MIN_SECS, 20, DROP_INTERVAL_MAX_SECS] {
            assert!(LootSettings { drop_interval_secs: secs, ..defaults() }.validate().is_ok());
        }
        for w in [WEIGHT_MIN, 50, WEIGHT_MAX] {
            assert!(LootSettings { barrier_weight: w, ..defaults() }.validate().is_ok());
        }
        for d in [BARRIER_DURATION_MIN_SECS, 30, BARRIER_DURATION_MAX_SECS] {
            assert!(LootSettings { barrier_duration_secs: d, ..defaults() }.validate().is_ok());
        }
    }

    #[test]
    fn validate_rejects_interval_out_of_range() {
        let err = LootSettings { drop_interval_secs: DROP_INTERVAL_MIN_SECS - 1, ..defaults() }
            .validate()
            .unwrap_err();
        assert_eq!(err.field, "drop_interval_secs");
        assert_eq!((err.min, err.max, err.requested), (5, 60, 4));

        let err = LootSettings { drop_interval_secs: DROP_INTERVAL_MAX_SECS + 1, ..defaults() }
            .validate()
            .unwrap_err();
        assert_eq!(err.requested, 61);
    }

    #[test]
    fn validate_rejects_weight_out_of_range() {
        let err = LootSettings { barrier_weight: 0, ..defaults() }.validate().unwrap_err();
        assert_eq!(err.field, "barrier_weight");
        assert_eq!((err.min, err.max, err.requested), (1, 100, 0));
    }

    #[test]
    fn validate_rejects_duration_out_of_range() {
        let err = LootSettings { barrier_duration_secs: 4, ..defaults() }.validate().unwrap_err();
        assert_eq!(err.field, "barrier_duration_secs");
        assert_eq!((err.min, err.max, err.requested), (5, 120, 4));

        let err = LootSettings { barrier_duration_secs: 121, ..defaults() }.validate().unwrap_err();
        assert_eq!(err.requested, 121);
    }

    #[test]
    fn a_single_item_can_never_oversubscribe() {
        // With one item capped at 100 the total cap is unreachable from the struct
        // alone; this documents that and guards the check for when item #2 lands.
        let s = LootSettings { barrier_weight: WEIGHT_MAX, ..defaults() };
        assert_eq!(s.subscribed_total(), WEIGHT_TOTAL_MAX);
        assert!(s.validate().is_ok());
        assert_eq!(s.nothing_rate(), 0.0);
    }

    #[test]
    fn weight_range_is_enforced_even_when_the_item_is_disabled() {
        let err = LootSettings { barrier_enabled: false, barrier_weight: 0, ..defaults() }
            .validate()
            .unwrap_err();
        assert_eq!(err.field, "barrier_weight");
    }
}
