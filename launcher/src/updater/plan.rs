//! The shape of one update job: an ordered list of steps, each weighted by the
//! real amount of work it represents.
//!
//! This exists so the progress display is driven by *data about the release*
//! rather than by constants compiled into the launcher. Today every job has one
//! component ("game" or "launcher") and so the plans are three and two steps
//! long respectively. That is not hardcoded anywhere: [`UpdatePlan::steps`] is
//! built from the release's asset size and its `files.json` manifest, which is
//! what makes phased multi-step updates and per-component updates an extension
//! of this module rather than a rewrite of the UI.
//!
//! Two numbers come out of a plan and they answer different questions:
//!
//! * [`UpdatePlan::label`] — the text. Names the phase, says which step of how
//!   many it is, and reports progress **within that step**, so the percentage
//!   restarts at each phase boundary.
//! * [`UpdatePlan::overall_fraction`] — the bar. A single weighted position
//!   across the whole job, which therefore never restarts and never goes
//!   backwards.
//!
//! They deliberately disagree: the text tells you where you are in the current
//! piece of work, the bar tells you how much of everything is left.

use std::fmt;

/// The kind of work a step performs. Ordered as they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Downloading,
    Installing,
    Verifying,
}

impl Phase {
    /// Verb shown to the user. Present participle so the label reads as a
    /// status rather than a command.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Downloading => "Downloading",
            Phase::Installing => "Installing",
            Phase::Verifying => "Verifying",
        }
    }

    /// Relative cost of one byte in this phase, used to weight the bar.
    ///
    /// A megabyte pulled over the network and a megabyte fed through sha256 are
    /// not the same amount of waiting, so a bar weighted by raw bytes would
    /// crawl through the download and then leap. These are deliberately coarse
    /// order-of-magnitude ratios (network is roughly ten times slower per byte
    /// than local extraction; hashing is faster still because it only reads).
    /// They are the one place to tune bar smoothness — the *relative* sizes
    /// within a phase always come from real manifest bytes and are unaffected.
    fn cost_per_byte(self) -> f64 {
        match self {
            Phase::Downloading => 10.0,
            Phase::Installing => 1.0,
            Phase::Verifying => 0.5,
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One unit of work in a plan: a phase applied to one component.
#[derive(Debug, Clone)]
pub struct Step {
    pub phase: Phase,
    /// Component this step operates on. One component today; the manifest is
    /// what will declare more of them.
    pub component: String,
    /// 1-based position of this component *within its phase*.
    pub component_index: usize,
    /// How many components this phase covers. `1` collapses the component
    /// fraction out of the label entirely.
    pub component_count: usize,
    /// Cost units, not bytes: real bytes scaled by [`Phase::cost_per_byte`].
    pub weight: u64,
}

/// An ordered, weighted description of everything one update will do.
#[derive(Debug, Clone)]
pub struct UpdatePlan {
    steps: Vec<Step>,
    /// Precomputed `steps.iter().map(|s| s.weight).sum()`, so the per-frame
    /// `overall_fraction` call is not re-summing the list on every redraw.
    total_weight: u64,
}

/// Assumed compression ratio when the real uncompressed size is not yet known.
///
/// Used only for the bar's weighting, and only until the release's `files.json`
/// is available — either fetched as a standalone asset up front, or read out of
/// the staging directory after extraction. A wrong guess here makes the bar
/// slightly uneven; it can never make a step report the wrong percentage,
/// because step percentages are measured against that step's own real total.
const ASSUMED_COMPRESSION_RATIO: f64 = 2.0;

/// Estimate uncompressed install bytes from the compressed download size, for
/// releases published before the standalone manifest asset existed.
pub fn estimate_installed_bytes(download_bytes: u64) -> u64 {
    (download_bytes as f64 * ASSUMED_COMPRESSION_RATIO) as u64
}

impl UpdatePlan {
    /// Build a plan from per-phase byte totals for a single component.
    ///
    /// `installed_bytes` covers both the extract and the hash pass: they move
    /// the same set of files, just at different speeds, which the phase cost
    /// coefficients account for.
    fn single_component(
        component: &str,
        phases: &[(Phase, u64)],
    ) -> Self {
        let steps: Vec<Step> = phases
            .iter()
            .map(|(phase, bytes)| Step {
                phase: *phase,
                component: component.to_string(),
                component_index: 1,
                component_count: 1,
                weight: (*bytes as f64 * phase.cost_per_byte()) as u64,
            })
            .collect();
        let total_weight = steps.iter().map(|s| s.weight).sum();
        Self {
            steps,
            total_weight,
        }
    }

    /// A game channel install or update: download the archive, extract it into
    /// staging, then verify staging against `files.json` before the swap.
    pub fn game(download_bytes: u64, installed_bytes: u64) -> Self {
        Self::single_component(
            "game",
            &[
                (Phase::Downloading, download_bytes),
                (Phase::Installing, installed_bytes),
                (Phase::Verifying, installed_bytes),
            ],
        )
    }

    /// A launcher self-update: download the asset, then swap the binary. There
    /// is no `files.json` for a single executable, so there is no verify step —
    /// the download's magic-byte check is the integrity gate.
    ///
    /// The swap is effectively instantaneous next to the download, so it gets a
    /// nominal weight rather than a measured one: without it the second step
    /// would own zero of the bar and the label would flash past at 100%.
    pub fn launcher(download_bytes: u64) -> Self {
        Self::single_component(
            "launcher",
            &[
                (Phase::Downloading, download_bytes),
                // ~2% of the download's cost — visible, but not a lie about how
                // long a rename takes.
                (Phase::Installing, download_bytes / 50),
            ],
        )
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Position of `phase` in this plan.
    ///
    /// The installer reports which *phase* it is in and the plan decides which
    /// step number that is, so the step ordering is defined in exactly one
    /// place. With one component a phase appears once; when components arrive
    /// this grows a `component` argument and the rest of the call sites are
    /// unaffected.
    pub fn index_of_phase(&self, phase: Phase) -> Option<usize> {
        self.steps.iter().position(|s| s.phase == phase)
    }

    /// Position of the bar across the entire job, in `0.0..=1.0`.
    ///
    /// `step_fraction` is progress within step `index` only. Steps before it
    /// count as complete, steps after it as untouched.
    pub fn overall_fraction(&self, index: usize, step_fraction: f32) -> f32 {
        if self.steps.is_empty() {
            return 0.0;
        }
        let step_fraction = step_fraction.clamp(0.0, 1.0) as f64;
        // Degenerate weights (a zero-byte release, or an asset whose size the
        // API did not report) would divide by zero. Fall back to treating every
        // step as equal — still monotonic, still correct at the boundaries.
        if self.total_weight == 0 {
            let per_step = 1.0 / self.steps.len() as f64;
            let done = index.min(self.steps.len()) as f64 * per_step;
            return (done + per_step * step_fraction).clamp(0.0, 1.0) as f32;
        }
        let done: u64 = self.steps.iter().take(index).map(|s| s.weight).sum();
        let current = self
            .steps
            .get(index)
            .map(|s| s.weight as f64 * step_fraction)
            .unwrap_or(0.0);
        (((done as f64) + current) / self.total_weight as f64).clamp(0.0, 1.0) as f32
    }

    /// The text drawn on the bar, e.g. `Downloading 1/3  47%`.
    ///
    /// With more than one component the component's own fraction is inserted:
    /// `Downloading 1/3  audio 3/10  47%`. The two fractions are both kept
    /// because they answer different questions — the first says how many kinds
    /// of work remain, the second how many pieces of *this* kind remain, and a
    /// ten-component download would otherwise look frozen at `1/3` throughout.
    pub fn label(&self, index: usize, step_fraction: f32) -> String {
        let Some(step) = self.steps.get(index) else {
            return String::new();
        };
        let pct = (step_fraction.clamp(0.0, 1.0) * 100.0).round() as u32;
        let position = format!("{}/{}", index + 1, self.steps.len());
        if step.component_count > 1 {
            format!(
                "{} {}  {} {}/{}  {}%",
                step.phase,
                position,
                step.component,
                step.component_index,
                step.component_count,
                pct
            )
        } else {
            format!("{} {}  {}%", step.phase, position, pct)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single-component text is exactly the agreed format: verb, step
    /// fraction, percent — no `1/1` component noise.
    #[test]
    fn single_component_label_matches_spec() {
        let plan = UpdatePlan::game(1000, 2000);
        assert_eq!(plan.label(0, 0.47), "Downloading 1/3  47%");
        assert_eq!(plan.label(1, 0.83), "Installing 2/3  83%");
        assert_eq!(plan.label(2, 0.12), "Verifying 3/3  12%");
    }

    /// A launcher plan is two steps, so the same code prints `1/2` and `2/2`
    /// without anything being told how long a self-update is.
    #[test]
    fn launcher_plan_has_two_steps() {
        let plan = UpdatePlan::launcher(5_000_000);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan.label(0, 0.10), "Downloading 1/2  10%");
        assert_eq!(plan.label(1, 0.20), "Installing 2/2  20%");
    }

    /// The component fraction appears only when there is more than one.
    #[test]
    fn multi_component_label_includes_component_fraction() {
        let plan = UpdatePlan {
            steps: vec![Step {
                phase: Phase::Downloading,
                component: "audio".into(),
                component_index: 3,
                component_count: 10,
                weight: 100,
            }],
            total_weight: 100,
        };
        assert_eq!(plan.label(0, 0.47), "Downloading 1/1  audio 3/10  47%");
    }

    /// The percentage is per-step, so it must clamp rather than run past 100.
    #[test]
    fn label_percent_clamps() {
        let plan = UpdatePlan::game(1000, 2000);
        assert_eq!(plan.label(0, 1.5), "Downloading 1/3  100%");
        assert_eq!(plan.label(0, -0.5), "Downloading 1/3  0%");
    }

    /// An index past the end yields no text rather than panicking — a late
    /// progress event must never take the UI down.
    #[test]
    fn label_out_of_range_is_empty() {
        let plan = UpdatePlan::game(1000, 2000);
        assert_eq!(plan.label(9, 0.5), "");
        assert_eq!(plan.overall_fraction(9, 0.5), 1.0);
    }

    /// The bar must never move backwards as the job advances, whatever the
    /// weights are.
    #[test]
    fn overall_fraction_is_monotonic() {
        let plan = UpdatePlan::game(50_000_000, 120_000_000);
        let mut last = -1.0_f32;
        for index in 0..plan.len() {
            for tenth in 0..=10 {
                let f = plan.overall_fraction(index, tenth as f32 / 10.0);
                assert!(f >= last, "went backwards at step {index}: {f} < {last}");
                last = f;
            }
        }
        assert!((last - 1.0).abs() < f32::EPSILON, "must finish full: {last}");
    }

    /// Downloading dominates the bar, because it dominates the wall clock. This
    /// is the whole reason weights exist rather than equal thirds.
    #[test]
    fn download_owns_most_of_the_bar() {
        // A typical release: ~50 MB compressed, ~120 MB installed.
        let plan = UpdatePlan::game(50_000_000, 120_000_000);
        let after_download = plan.overall_fraction(1, 0.0);
        assert!(
            after_download > 0.5,
            "download should own over half the bar, got {after_download}"
        );
    }

    /// Zero weights (a release whose asset size the API omitted) must degrade
    /// to an even split instead of dividing by zero.
    #[test]
    fn zero_weight_plan_falls_back_to_even_steps() {
        let plan = UpdatePlan::game(0, 0);
        assert_eq!(plan.overall_fraction(0, 0.0), 0.0);
        assert!((plan.overall_fraction(1, 0.0) - 1.0 / 3.0).abs() < 1e-6);
        assert!((plan.overall_fraction(2, 1.0) - 1.0).abs() < 1e-6);
    }

    /// The compressed-size fallback still produces a usable three-step plan
    /// whose download step dominates — the estimate only has to be the right
    /// order of magnitude.
    #[test]
    fn estimated_plan_is_still_sensible() {
        let download = 10_000_000;
        let plan = UpdatePlan::game(download, estimate_installed_bytes(download));
        assert_eq!(plan.len(), 3);
        assert!(plan.overall_fraction(1, 0.0) > 0.5);
    }
}
