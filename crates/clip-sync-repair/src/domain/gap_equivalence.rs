//! Gap content-equivalence gate — "does this gap actually need patching?" (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate).
//!
//! **Silence-character classification.** A scanned silent run in A is worth repairing only when A's signal
//! genuinely *died* (a dropout) **and** B carries the missing content. The two signals that decide it, both
//! already in the fingerprint:
//!
//! - **A-side (`a_rms` vs the recording's `noise_floor_db`):** a true dropout sits **far below** A's own noise
//!   floor (the signal is gone); a genuine quiet passage sits **at** the noise floor (room tone). Measuring
//!   *relative to the noise floor* makes the threshold **self-calibrating** — no hard-coded absolute dB.
//! - **B-side (`donor_silence_fraction`):** if B is silent at the nominal span there is nothing to fill with.
//!   A B block counts as silent when [`BlockLevel::silent`] (scanner predicate) **or** quieter than
//!   A's gap floor — so digital silence / abs-floor quiet is not misread as occupied via a strict
//!   `rms_db < gap_floor` compare at the −120 floor.
//!
//! Empirically (licensed media): the two silence signals separate cleanly (dropouts ≥35 dB below noise floor,
//! `donor_silence` bimodal at ~0 vs ~1) where the seam/lag approach failed — the recordings drift, so "B matches
//! A" is never a lag-0 match. This gate replaces that approach.

use serde::{Deserialize, Serialize};

use crate::domain::policies::{BlockLevel, BLOCK_LEVEL_FLOOR_DB};

/// Context window (seconds) each side of a gap used to estimate A's local noise floor from the scan's
/// per-block level timeline — blocks in `[a_start − ctx, a_end + ctx]` but **outside** the gap. The
/// scan-time analogue of the fingerprint's `gap_signature_context_secs`.
pub const EQUIVALENCE_CONTEXT_SECS: f64 = 2.0;

/// Tunable thresholds for the equivalence gate (all overridable; gate is **off by default**).
#[derive(Debug, Clone, Copy)]
pub struct GapEquivalenceParams {
    /// Master on/off. When `false`, every gap classifies `NotEvaluated` (keep) — zero behavior change.
    pub enabled: bool,
    /// A counts as a **dropout** when `a_rms_db < noise_floor_db − dropout_margin_db` (default `35.0`).
    /// Relative to the recording's own noise floor, so it self-calibrates across noisy/clean sources.
    pub dropout_margin_db: f64,
    /// B counts as **occupied** when `donor_silence_fraction < donor_silence_thresh` (default `0.5`, the
    /// program-quiet valley); at/above ⇒ B silent ⇒ nothing to fill.
    pub donor_silence_thresh: f64,
}

impl Default for GapEquivalenceParams {
    fn default() -> Self {
        Self {
            enabled: false,
            dropout_margin_db: 35.0,
            donor_silence_thresh: 0.5,
        }
    }
}

/// Vocabulary for the gate — the reason a gap does or doesn't need patching. These are the
/// **scan-time silence-character cells** in [`docs/dev/gap-vocabulary.md`] (§ *Silence-character pre-gate*),
/// a pre-filter that runs before the seam/donor cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapEquivalenceClass {
    /// A's signal died (RMS ≥ `dropout_margin_db` below the recording's noise floor) **and** B carries content
    /// — a real dropout with a fill source. **Keep** — *not a skip cell*: the gap proceeds into the normal
    /// seam/donor cells (Bracket-patch / Silence-splice / …).
    RepairableDropout,
    /// B is silent at the nominal span (`donor_silence ≥ thresh`) — nothing to fill with, patching can't help.
    /// **Drop.** (Both "A dropped out but the donor is also dead" and "quiet in both" land here.) This is the
    /// **plan-time detection of the Program-quiet cell** — the same disposition the patch path skips as
    /// `GapPatchSkipReason::ProgramQuiet`, surfaced before decode as `GapFillSkipReason::AlreadyMatchesReference`.
    SharedSilence,
    /// A is only ambient room tone (near its own noise floor), not a signal failure, though B has content — a
    /// genuine quiet passage, not a dropout. **Drop** (don't inject content into intentional quiet). A cell with
    /// no seam/donor counterpart — decided on A's own character, not B's donor state.
    AmbientQuiet,
    /// Gate disabled or a required signal missing — **keep** (no decision made). Also the `Default`:
    /// the only variant that asserts nothing about the audio, so a default-constructed verdict can
    /// never fabricate a drop.
    #[default]
    NotEvaluated,
}

impl GapEquivalenceClass {
    /// Whether this gap should be dropped from the fill plan (no patching needed).
    pub fn drops(self) -> bool {
        matches!(self, Self::SharedSilence | Self::AmbientQuiet)
    }
}

/// The gate's per-gap readout: the class + the signals it was derived from (for tuning + reporting).
///
/// `Default` is `NotEvaluated` with every signal absent, so tests and constructors can spread
/// `..Default::default()` and stay correct as provenance fields are added.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GapEquivalenceVerdict {
    pub class: GapEquivalenceClass,
    /// `class.drops()` — surfaced so consumers don't re-derive it.
    pub drop: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_rms_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub noise_floor_db: Option<f64>,
    /// `a_gap_rms_db − noise_floor_db` — how far below the noise floor A's gap sits (the self-calibrated signal).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_below_noise_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_silence_fraction: Option<f64>,
    /// The floor `donor_silence_fraction` was measured against. **Provenance, not a classification
    /// input** — nothing reads it to decide a class; it is recorded so the donor fraction can be
    /// audited after the fact.
    ///
    /// The two equivalence front-ends define this differently, and the difference is decision-sized
    /// (~20 dB observed): the scan path uses the loudest **silent** A block in the gap (immune to
    /// hold-bridging and edge refinement — the F2/R1 definition), while the fingerprint path uses
    /// the loudest content **anywhere** in the gap span, unfiltered. Recording it is what makes the
    /// two comparable at all; before this it was derivable only as an arithmetic bound.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gap_floor_db: Option<f64>,
    /// Count of A blocks inside the gap that passed the silence test — the population behind
    /// `a_gap_rms_db` (energy mean) and `gap_floor_db` (max). Provenance only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_silent_blocks: Option<usize>,
    /// Blocks whose centre falls inside the gap (silent or not) — the denominator behind
    /// [`Self::a_gap_silent_blocks`]. With [`EquivalenceMeasurement::bin_ms`],
    /// `total × bin_ms ≈ span` is the corpus-wide bin-width check (I1 class). Provenance only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_total_blocks: Option<usize>,
    /// Silent / total donor blocks behind `donor_silence_fraction`. Provenance only — a fraction
    /// alone cannot distinguish `1/10` from `1.1/11`, which matters when comparing paths that bin
    /// the same span differently.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_silent_blocks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_total_blocks: Option<usize>,
    /// The one measurement recipe that produced this verdict — see [`EquivalenceMeasurement`].
    /// Absent on pre-Track-B corpora and when the scan path has no level stream to derive `bin_ms`
    /// from. Provenance only; never classified on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub measurement: Option<EquivalenceMeasurement>,
    /// Candidate **noise floors** over a grid of context windows × bin sizes — see [`NoiseFloorProbe`].
    /// Provenance only; empty (and omitted) unless a front-end computes them. **Retained** for I2
    /// residual attribution.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub noise_floor_probes: Vec<NoiseFloorProbe>,
}

/// Which span a front-end measured on — A-side window or donor window.
///
/// One enum for both [`EquivalenceMeasurement::a_span`] and
/// [`EquivalenceMeasurement::donor_span`]: today A always emits [`Core`](SpanKind::Core), and the
/// live residual is donor **core vs nominal**. A single-variant A-side enum would have to widen the
/// first time A splits — exactly the event the field exists to make visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Block-confirmed / offset-mapped **core** (scan's A window and donor window; diagnostic A).
    Core,
    /// Nominal `b_mapped` span (diagnostic donor).
    Nominal,
}

/// The recipe that classified one [`GapEquivalenceVerdict`] — permanent replacement for the deleted
/// `silent_core_probes` grid. Nested so it stays visually distinct from the retained
/// [`NoiseFloorProbe`] **candidate** grid (I2).
///
/// See `docs/dev/TEMP-fingerprint-provenance-plan.md` §3a. Provenance only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquivalenceMeasurement {
    /// Context half-width in seconds each side of the gap used for `noise_floor_db`.
    pub context_secs: f64,
    /// Bin width in milliseconds actually measured (from the level stream / configured overlay).
    pub bin_ms: u64,
    pub reduction: ChannelReduction,
    pub a_span: SpanKind,
    pub donor_span: SpanKind,
}

/// How a bin's multiple channels are collapsed to one level before it is expressed in dB — the
/// **third** F15 noise-floor variable, and the one the `(2 s, scan_block_ms)` probe row exposed by
/// failing to reproduce the scan floor on any gap.
///
/// The two differ by the zero-lag cross-correlation between the channels. With mean pairwise
/// correlation `ρ̄` over `N` channels the ratio is `(1 + (N−1)·ρ̄) / N`: identical channels agree
/// exactly, decorrelated ones differ by `10·log10(N)` (7.78 dB at 6 channels), and anti-correlated
/// ones diverge without bound. Cauchy–Schwarz makes [`Downmix`](ChannelReduction::Downmix) ≤
/// [`Interleaved`](ChannelReduction::Interleaved) *always*, so the sign of the gap carries no
/// information — only its size does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChannelReduction {
    /// Average the channels per frame, **then** square: an amplitude mean, i.e. a mono downmix. What
    /// the diagnostic path's `mono_rms` / `interleaved_to_mono` do. The variant existing dumps recorded, so
    /// it is the `serde` default.
    #[default]
    Downmix,
    /// RMS over all interleaved samples: a power mean across channels, independent of inter-channel
    /// phase. What the scan path's `block_rms_db` does via `rms_interleaved`.
    Interleaved,
}

/// A candidate `noise_floor_db` — median dB over the context bins **outside** the gap — measured at one
/// `(context window, bin size, channel reduction)` combination. This is the **one surviving** axis
/// between the two front-ends (I2): both now bin at `scan_block_ms` and reduce interleaved, but the
/// context window is ±2.0 s (scan) vs ±3.0 s (diagnostic). Residual median **0.606 dB**, still biasing
/// the diagnostic side **lower**, which shrinks `a_below_noise` and pushes gaps out of `repairable_dropout` — the safe
/// direction. Kept deliberately un-converged; these probes are what make it attributable.
///
/// **Provenance only.** Emitted over a grid so the variables can be separated: the probe at scan's own
/// `(2 s, scan_block_ms, Interleaved)` should reproduce `scan_equivalence.noise_floor_db`, and the
/// crosses isolate each variable's contribution. The window/bin-only grid did *not* reproduce it —
/// undershooting 3.13–7.96 dB uniformly — which is what added [`ChannelReduction`] as the third
/// dimension. A residual after all three is most likely the excluded span (the diagnostic path excludes the *refined*
/// gap, scan the block-confirmed *core*).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseFloorProbe {
    /// Context half-width in seconds each side of the gap.
    pub context_secs: f64,
    /// Bin width in milliseconds the context was binned at.
    pub bin_ms: u64,
    /// How each bin's channels were collapsed to one level. Defaults to
    /// [`Downmix`](ChannelReduction::Downmix) when absent, which is what dumps predating this field
    /// recorded.
    #[serde(default)]
    pub reduction: ChannelReduction,
    /// Median dB over the context bins — the candidate floor. `None` when the context was empty,
    /// rather than the `median()` helper's −120 placeholder, so "no context" is distinguishable from
    /// "silent context".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub floor_db: Option<f64>,
    /// Context bins behind the median — the population. A median over 40 bins and one over 400 are
    /// not equally trustworthy, and the two windows differ in exactly this.
    pub context_bins: usize,
}

impl GapEquivalenceVerdict {
    fn of(class: GapEquivalenceClass, a: Option<f64>, nf: Option<f64>, ds: Option<f64>) -> Self {
        Self {
            class,
            drop: class.drops(),
            a_gap_rms_db: a,
            noise_floor_db: nf,
            a_below_noise_db: match (a, nf) {
                (Some(a), Some(nf)) => Some(a - nf),
                _ => None,
            },
            donor_silence_fraction: ds,
            gap_floor_db: None,
            a_gap_silent_blocks: None,
            a_gap_total_blocks: None,
            donor_silent_blocks: None,
            donor_total_blocks: None,
            measurement: None,
            noise_floor_probes: Vec::new(),
        }
    }

    /// Attach population provenance (A silent/total + donor silent/total). Never changes `class` or
    /// `drop`. A is a tuple matching the donor shape — see
    /// `docs/dev/TEMP-fingerprint-provenance-plan.md` §3b.
    #[must_use]
    pub fn with_scan_provenance(
        mut self,
        gap_floor_db: Option<f64>,
        a_gap_blocks: (usize, usize),
        donor_blocks: Option<(usize, usize)>,
    ) -> Self {
        self.gap_floor_db = gap_floor_db;
        let (silent, total) = a_gap_blocks;
        self.a_gap_silent_blocks = Some(silent);
        self.a_gap_total_blocks = Some(total);
        (self.donor_silent_blocks, self.donor_total_blocks) = match donor_blocks {
            Some((silent, total)) => (Some(silent), Some(total)),
            None => (None, None),
        };
        self
    }

    // `with_gap_floor_db` (attach `levels.gap_floor_db` — the whole-span content peak) was removed with
    // F15 fix 1. The diagnostic path now measures its own silent-core floor and carries it via
    // `with_scan_provenance`; re-attaching the levels number would overwrite the fix with the statistic it
    // exists to replace. `levels.gap_floor_db` is still dumped in its own block.

    /// Attach the live measurement recipe. Never changes `class` or `drop`.
    #[must_use]
    pub fn with_measurement(mut self, measurement: EquivalenceMeasurement) -> Self {
        self.measurement = Some(measurement);
        self
    }

    /// Attach candidate noise floors. Never changes `class` or `drop`. **Retained** for I2
    /// attribution.
    #[must_use]
    pub fn with_noise_floor_probes(mut self, probes: Vec<NoiseFloorProbe>) -> Self {
        self.noise_floor_probes = probes;
        self
    }
}

/// After occupancy and donor silence both honor [`BlockLevel::silent`], a
/// donor-silent absolute read (`!b_has_energy`) must not disagree with a high
/// `donor_silence_fraction`. Missing donor fraction → vacuously true (nothing to compare).
///
/// Does **not** change plan precedence (`NotFillable` still wins); it only surfaces the
/// post-F1 consistency invariant (and catches regressions that reintroduce rms-only donor scoring).
pub fn occupancy_agrees_with_donor_silence(
    b_has_energy: bool,
    donor_silence_fraction: Option<f64>,
    donor_silence_thresh: f64,
) -> bool {
    match donor_silence_fraction {
        None => true,
        Some(ds) if !b_has_energy => ds >= donor_silence_thresh,
        Some(_) => true,
    }
}

/// Classify one gap from its silence signals. Pure — no I/O, no measurement.
///
/// - `NotEvaluated` when the gate is off or any signal is missing.
/// - `SharedSilence` when B is silent (nothing to fill).
/// - `RepairableDropout` when A's signal died (below the noise floor by the margin) and B is occupied.
/// - `AmbientQuiet` when B is occupied but A is only room tone (not a dropout).
pub fn classify_gap_equivalence(
    a_gap_rms_db: Option<f64>,
    noise_floor_db: Option<f64>,
    donor_silence_fraction: Option<f64>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    if !params.enabled {
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        );
    }
    let (Some(a), Some(nf), Some(ds)) = (a_gap_rms_db, noise_floor_db, donor_silence_fraction)
    else {
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        );
    };
    let is_dropout = a < nf - params.dropout_margin_db;
    let b_occupied = ds < params.donor_silence_thresh;
    let class = match (is_dropout, b_occupied) {
        (true, true) => GapEquivalenceClass::RepairableDropout,
        (_, false) => GapEquivalenceClass::SharedSilence,
        (false, true) => GapEquivalenceClass::AmbientQuiet,
    };
    GapEquivalenceVerdict::of(class, a_gap_rms_db, noise_floor_db, donor_silence_fraction)
}

/// A block's timeline center (used for gap/context membership). Block duration is the
/// `scan_block_ms` recipe knob — do not restate it as a literal here; that is how this comment came
/// to claim 250 ms long after the default moved to 100.
fn block_center(b: &BlockLevel) -> f64 {
    (b.start_secs + b.end_secs) / 2.0
}

/// Combine per-block dB levels into one aggregate RMS in dB (energy mean of the blocks). `None` when empty.
pub(crate) fn aggregate_rms_db(levels: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum_sq = 0.0f64;
    let mut n = 0usize;
    for db in levels {
        let amp = 10f64.powf(db / 20.0);
        sum_sq += amp * amp;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let rms = (sum_sq / n as f64).sqrt();
    Some(if rms <= 1e-9 {
        BLOCK_LEVEL_FLOOR_DB
    } else {
        20.0 * rms.log10()
    })
}

/// Median of a set of dB values (used for A's local noise floor). `None` when empty.
fn median_db(mut vals: Vec<f64>) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(vals[vals.len() / 2])
}

/// Derive the per-gap silence-character signals from the scan's per-block level timelines and classify
/// (`docs/gap-scan.md`; vocabulary in `docs/dev/gap-vocabulary.md` § Silence-character pre-gate). All three signals are the scan-block
/// analogues of the fingerprint's finer-bin reads:
///
/// - **noise floor** — median dB of A blocks in `±`[`EQUIVALENCE_CONTEXT_SECS`] around the gap, **excluding**
///   blocks inside it (the recording's own floor; self-calibrating).
/// - **A gap RMS** — aggregate RMS of A's **silent** blocks inside the gap (hold-bridged non-silent
///   blocks inside the core interval are excluded so they cannot inflate dropout depth).
/// - **donor silence fraction** — fraction of B blocks over the nominal mapped span that are
///   scanner-silent ([`BlockLevel::silent`]) or quieter than A's gap floor.
///
/// `b_levels`/`b_mapped` are `None` when B was not scanned (missing/unaligned) ⇒ donor signal absent ⇒
/// `NotEvaluated`. Pure — no I/O.
///
/// Also records measurement **provenance** on the verdict (`gap_floor_db`, A/donor block populations,
/// [`EquivalenceMeasurement`]) — recorded, never classified on.
pub fn derive_gap_equivalence(
    a_levels: &[BlockLevel],
    a_start_secs: f64,
    a_end_secs: f64,
    b_levels: Option<&[BlockLevel]>,
    b_mapped: Option<(f64, f64)>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    let centre_in_gap = |b: &BlockLevel| {
        let c = block_center(b);
        c >= a_start_secs && c < a_end_secs
    };
    // Silent A gap blocks only — hold can place non-silent levels inside the core interval.
    let gap_silent_blocks = || a_levels.iter().filter(|b| b.silent && centre_in_gap(b));
    // All A gap blocks (silent or not) — denominator for `a_gap_total_blocks` / the I1 bin check.
    let a_gap_total_blocks = a_levels.iter().filter(|b| centre_in_gap(b)).count();
    let a_gap_silent_blocks = gap_silent_blocks().count();
    let a_gap_rms_db = aggregate_rms_db(gap_silent_blocks().map(|b| b.rms_db));
    let gap_floor_db = gap_silent_blocks()
        .map(|b| b.rms_db)
        .fold(f64::NEG_INFINITY, f64::max);

    // A context blocks: within the context window but outside the gap → median = local noise floor.
    let ctx_lo = a_start_secs - EQUIVALENCE_CONTEXT_SECS;
    let ctx_hi = a_end_secs + EQUIVALENCE_CONTEXT_SECS;
    let noise_floor_db = median_db(
        a_levels
            .iter()
            .filter(|b| {
                let c = block_center(b);
                c >= ctx_lo && c < ctx_hi && !(c >= a_start_secs && c < a_end_secs)
            })
            .map(|b| b.rms_db)
            .collect(),
    );

    // Donor silence: scanner silent bit (peak/per-channel, abs floor baked in) OR quieter than
    // A's gap floor. Never re-threshold rms alone against the floor — digitally silent blocks
    // sit at BLOCK_LEVEL_FLOOR_DB and `rms < gap_floor` is false when both are −120.
    let gap_floor = gap_floor_db.is_finite().then_some(gap_floor_db);
    let donor_blocks = match (b_levels, b_mapped) {
        (Some(bl), Some((b_start, b_end))) => {
            let mut total = 0usize;
            let mut silent = 0usize;
            for b in bl.iter().filter(|b| {
                let c = block_center(b);
                c >= b_start && c < b_end
            }) {
                total += 1;
                if b.silent || gap_floor.is_some_and(|g| b.rms_db < g) {
                    silent += 1;
                }
            }
            Some((silent, total))
        }
        _ => None,
    };
    let donor_silence_fraction =
        donor_blocks.and_then(|(silent, total)| (total > 0).then(|| silent as f64 / total as f64));

    let mut verdict = classify_gap_equivalence(
        a_gap_rms_db,
        noise_floor_db,
        donor_silence_fraction,
        params,
    )
    .with_scan_provenance(
        gap_floor,
        (a_gap_silent_blocks, a_gap_total_blocks),
        donor_blocks,
    );
    // `bin_ms` is a property of the level stream, not the gap population — any block's width works.
    // Empty `a_levels` ⇒ no measurement (do not invent a bin).
    if let Some(b) = a_levels.first() {
        let bin_ms = ((b.end_secs - b.start_secs) * 1000.0).round().max(0.0) as u64;
        verdict = verdict.with_measurement(EquivalenceMeasurement {
            context_secs: EQUIVALENCE_CONTEXT_SECS,
            bin_ms,
            reduction: ChannelReduction::Interleaved,
            a_span: SpanKind::Core,
            donor_span: SpanKind::Core,
        });
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;
    use GapEquivalenceClass::*;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
    }

    fn class(a: f64, nf: f64, ds: f64) -> GapEquivalenceClass {
        classify_gap_equivalence(Some(a), Some(nf), Some(ds), &on()).class
    }

    /// The four measured licensed-media cases (noise floor ~−45 to −70; margin 35, donor thresh 0.5).
    #[test]
    fn measured_cases_classify_as_ground_truth() {
        // Repairable dropout: a_rms −106, noise_floor −47 ⇒ 59 dB below; donor 0.0.
        assert_eq!(class(-106.0, -47.0, 0.0), RepairableDropout);
        // Mutual silence: a_rms −81, noise_floor −71 ⇒ 10 dB below (not a dropout); donor 0.92 (B silent).
        assert_eq!(class(-81.0, -71.0, 0.92), SharedSilence);
        // Deep A but B dead (intro/tail): a_rms −108, noise_floor −46 ⇒ dropout, but donor 1.0 ⇒ nothing to fill.
        assert_eq!(class(-108.0, -46.0, 1.0), SharedSilence);
    }

    /// Ambient A with an occupied donor is a genuine quiet passage → drop (not a dropout).
    #[test]
    fn ambient_with_occupied_donor_is_quiet_passage() {
        assert_eq!(class(-80.0, -70.0, 0.0), AmbientQuiet); // only 10 dB below floor
        assert!(AmbientQuiet.drops());
    }

    /// The margin is self-calibrating: the same 40 dB drop is a dropout under both a low and a high noise floor.
    #[test]
    fn margin_is_relative_to_noise_floor() {
        assert_eq!(class(-100.0, -60.0, 0.0), RepairableDropout); // 40 dB below a −60 floor
        assert_eq!(class(-120.0, -80.0, 0.0), RepairableDropout); // 40 dB below a −80 floor
        assert_eq!(class(-90.0, -60.0, 0.0), AmbientQuiet); // only 30 dB below ⇒ not a dropout
    }

    #[test]
    fn drops_only_the_two_silence_classes() {
        assert!(!RepairableDropout.drops());
        assert!(!NotEvaluated.drops());
        assert!(SharedSilence.drops());
        assert!(AmbientQuiet.drops());
    }

    #[test]
    fn disabled_or_missing_signal_is_not_evaluated() {
        assert_eq!(
            classify_gap_equivalence(
                Some(-106.0),
                Some(-47.0),
                Some(0.0),
                &GapEquivalenceParams::default()
            )
            .class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(None, Some(-47.0), Some(0.0), &on()).class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(Some(-106.0), Some(-47.0), None, &on()).class,
            NotEvaluated
        );
    }

    #[test]
    fn verdict_reports_a_below_noise() {
        let v = classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on());
        assert_eq!(v.a_below_noise_db, Some(-59.0));
        assert!(!v.drop && v.class == RepairableDropout);
    }

    #[test]
    fn occupancy_agrees_with_donor_when_both_say_silent() {
        assert!(occupancy_agrees_with_donor_silence(false, Some(1.0), 0.5));
        assert!(occupancy_agrees_with_donor_silence(false, Some(0.5), 0.5));
        assert!(occupancy_agrees_with_donor_silence(true, Some(0.0), 0.5));
        assert!(occupancy_agrees_with_donor_silence(false, None, 0.5));
    }

    #[test]
    fn occupancy_disagrees_when_absolute_silent_but_donor_occupied() {
        // Pre-F1 E1 shape: absolute silent + donor_silence 0.0.
        assert!(!occupancy_agrees_with_donor_silence(false, Some(0.0), 0.5));
        assert!(!occupancy_agrees_with_donor_silence(false, Some(0.49), 0.5));
    }

    // --- derive_gap_equivalence (scan-block timelines → signals → classification) --------------------

    /// One 250 ms block at `[t, t+0.25)` with level `db`. Gap-interior test blocks default to silent.
    fn blk(t: f64, db: f64) -> BlockLevel {
        blk_silent(t, db, true)
    }

    fn blk_silent(t: f64, db: f64, silent: bool) -> BlockLevel {
        BlockLevel {
            start_secs: t,
            end_secs: t + 0.25,
            rms_db: db,
            silent,
        }
    }

    /// A dropout gap (A blocks far below a −50 floor) with an occupied B donor → RepairableDropout, and
    /// the derived signals match the block inputs (context noise floor = median of the flanking blocks).
    #[test]
    fn derive_dropout_with_occupied_donor() {
        // Context blocks at −50 dB flank a 0.5 s gap of silence-floored blocks; B is loud across the span.
        let a = vec![
            blk(9.5, -50.0),
            blk(9.75, -50.0),
            blk(10.0, -119.0),
            blk(10.25, -119.0),
            blk(10.5, -50.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, RepairableDropout);
        assert_eq!(v.noise_floor_db, Some(-50.0));
        assert_eq!(v.donor_silence_fraction, Some(0.0));
        assert!(v.a_gap_rms_db.unwrap() < -100.0, "{v:?}");
    }

    /// A dropout on A but B is silent over the mapped span → SharedSilence (nothing to fill).
    #[test]
    fn derive_dropout_with_silent_donor_is_shared_silence() {
        let a = vec![
            blk(9.5, -48.0),
            blk(9.75, -48.0),
            blk(10.0, -100.0),
            blk(10.25, -100.0),
            blk(10.5, -48.0),
        ];
        let b = vec![blk(10.0, -120.0), blk(10.25, -120.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence);
        assert_eq!(v.donor_silence_fraction, Some(1.0));
        assert!(v.drop);
    }

    /// Digitally silent / abs-floor-quiet B must not read occupied via `rms < gap_floor` alone (F1/R1).
    #[test]
    fn derive_donor_silent_bit_counts_even_when_rms_equals_gap_floor() {
        // A gap floor −120; B at −120 with silent=true (scanner). Strict `rms < thresh` would fail.
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -120.0),
            blk(10.25, -120.0),
            blk(10.5, -45.0),
        ];
        let b = vec![blk(10.0, -120.0), blk(10.25, -120.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0));
    }

    /// Scanner-silent dither below the abs floor (rms above A's gap floor) still counts as donor-silent.
    #[test]
    fn derive_donor_uses_silent_bit_for_abs_floor_quiet() {
        // A gap floor ≈ −101.5; B dither at −98.8 marked silent by the scanner abs floor.
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -101.5),
            blk(10.25, -101.5),
            blk(10.5, -45.0),
        ];
        let b = vec![
            blk_silent(10.0, -98.8, true),
            blk_silent(10.25, -98.8, true),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0));
    }

    /// Hold-bridged loud block inside the core must not inflate A dropout depth (F2).
    #[test]
    fn derive_ignores_non_silent_blocks_inside_gap_core() {
        let a = vec![
            blk(9.5, -45.0),
            blk(9.75, -45.0),
            blk(10.0, -101.0),
            blk_silent(10.25, -52.0, false), // bridged noise
            blk(10.5, -101.0),
            blk(10.75, -45.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
            blk_silent(10.5, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.75, Some(&b), Some((10.0, 10.75)), &on());
        assert_eq!(v.class, RepairableDropout, "{v:?}");
        assert!(
            v.a_gap_rms_db.unwrap() < -90.0,
            "silent-only aggregate must stay deep, got {v:?}"
        );
    }

    /// Room-tone gap (A only a few dB below its floor) with an occupied donor → AmbientQuiet (drop, but
    /// not a dropout) — the self-calibrating A-side at scan-block granularity.
    #[test]
    fn derive_roomtone_with_occupied_donor_is_ambient_quiet() {
        let a = vec![
            blk(9.5, -47.0),
            blk(9.75, -47.0),
            blk(10.0, -52.0),
            blk(10.25, -52.0),
            blk(10.5, -47.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, AmbientQuiet);
        assert!(v.drop);
    }

    /// No B timeline (unscanned/unaligned) ⇒ donor signal absent ⇒ NotEvaluated (conservative keep).
    #[test]
    fn derive_without_b_levels_is_not_evaluated() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, None, None, &on());
        assert_eq!(v.class, NotEvaluated);
    }

    /// Track B: `bin_ms` from any block width; A total = centres in gap; measurement present.
    #[test]
    fn derive_attaches_measurement_and_a_gap_total_from_levels() {
        let a = vec![
            blk(9.75, -48.0),
            blk(10.0, -119.0),
            blk(10.25, -119.0),
            blk(10.5, -48.0),
        ];
        let b = vec![
            blk_silent(10.0, -20.0, false),
            blk_silent(10.25, -20.0, false),
        ];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        let m = v.measurement.expect("levels present ⇒ measurement");
        assert_eq!(m.bin_ms, 250);
        assert!((m.context_secs - EQUIVALENCE_CONTEXT_SECS).abs() < f64::EPSILON);
        assert_eq!(m.reduction, ChannelReduction::Interleaved);
        assert_eq!(m.a_span, SpanKind::Core);
        assert_eq!(m.donor_span, SpanKind::Core);
        assert_eq!(v.a_gap_silent_blocks, Some(2));
        assert_eq!(v.a_gap_total_blocks, Some(2));
        assert_eq!(v.donor_silent_blocks, Some(0));
        assert_eq!(v.donor_total_blocks, Some(2));
    }

    /// Track B: empty level stream ⇒ no invented `bin_ms` / no measurement object.
    #[test]
    fn derive_with_empty_levels_omits_measurement() {
        let v = derive_gap_equivalence(&[], 10.0, 10.5, None, None, &on());
        assert!(v.measurement.is_none(), "{v:?}");
        assert_eq!(v.a_gap_total_blocks, Some(0));
        assert_eq!(v.a_gap_silent_blocks, Some(0));
    }

    /// The gate is off by default: even a clean dropout classifies NotEvaluated (advisory computes with
    /// `enabled: true` explicitly; the plan-drop flag is separate).
    #[test]
    fn derive_respects_disabled_params() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let b = vec![blk_silent(10.0, -20.0, false)];
        let v = derive_gap_equivalence(
            &a,
            10.0,
            10.5,
            Some(&b),
            Some((10.0, 10.5)),
            &GapEquivalenceParams::default(),
        );
        assert_eq!(v.class, NotEvaluated);
    }

    // --- Scanner → levels → occupancy/donor (production recipe; not hand-built BlockLevels) -----

    fn mono_pcm(rate: u32, samples: Vec<f32>) -> crate::domain::pcm::InterleavedPcm {
        crate::domain::pcm::InterleavedPcm {
            sample_rate: rate,
            channels: 1,
            samples,
        }
    }

    fn sine_samples(rate: u32, secs: f64) -> Vec<f32> {
        let count = (rate as f64 * secs).round() as usize;
        (0..count)
            .map(|i| f32::sin(i as f32 * 0.3) * 0.244)
            .collect()
    }

    fn scan_levels(
        samples: Vec<f32>,
        abs_floor: f32,
    ) -> (Vec<crate::domain::policies::SilentRun>, Vec<BlockLevel>) {
        let rate = 11_025u32;
        let mut scanner =
            crate::domain::policies::SilenceRunScanner::new(0.25, 0.01, 1.0, 0, abs_floor)
                .retain_block_levels();
        scanner.feed(&mono_pcm(rate, samples), 0.0);
        scanner.finish_with_levels()
    }

    #[test]
    fn scanner_pipeline_digital_silence_occupancy_and_donor_agree() {
        use crate::domain::cross_check::b_has_energy_from_levels;

        let abs = 33.0 / 32767.0;
        let rate = 11_025u32;
        // A: loud shoulders + 2 s digital silence (noise-floor context present).
        let mut a = sine_samples(rate, 2.0);
        a.extend(std::iter::repeat_n(0.0f32, rate as usize * 2));
        a.extend(sine_samples(rate, 2.0));
        let b = vec![0.0f32; rate as usize * 6];

        let (runs, a_levels) = scan_levels(a, abs);
        let (_, b_levels) = scan_levels(b, abs);
        assert_eq!(runs.len(), 1, "expected one silent run on A");
        let core_s = runs[0].core_start_secs;
        let core_e = runs[0].core_end_secs;

        assert!(
            !b_has_energy_from_levels(&b_levels, core_s, core_e),
            "digitally silent B must be unoccupied"
        );
        let v = derive_gap_equivalence(
            &a_levels,
            core_s,
            core_e,
            Some(&b_levels),
            Some((core_s, core_e)),
            &on(),
        );
        assert_eq!(v.donor_silence_fraction, Some(1.0), "{v:?}");
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert!(occupancy_agrees_with_donor_silence(
            false,
            v.donor_silence_fraction,
            0.5
        ));
    }

    #[test]
    fn scanner_pipeline_abs_floor_dither_is_donor_silent_not_occupied() {
        use crate::domain::cross_check::b_has_energy_from_levels;

        let abs = 33.0 / 32767.0;
        let rate = 11_025u32;
        let mut a = sine_samples(rate, 2.0);
        a.extend(std::iter::repeat_n(0.0f32, rate as usize * 2));
        a.extend(sine_samples(rate, 2.0));
        // B: ±1/32767 dither — quieter than abs floor peak check, louder than digital −120 gap floor.
        let dither = 1.0f32 / 32767.0;
        let b: Vec<f32> = (0..rate as usize * 6)
            .map(|i| if i % 2 == 0 { dither } else { -dither })
            .collect();

        let (runs, a_levels) = scan_levels(a, abs);
        let (_, b_levels) = scan_levels(b, abs);
        let core_s = runs[0].core_start_secs;
        let core_e = runs[0].core_end_secs;

        assert!(
            b_levels.iter().any(|l| {
                let c = (l.start_secs + l.end_secs) / 2.0;
                c >= core_s && c < core_e && l.silent
            }),
            "scanner must mark dither blocks silent under abs floor"
        );
        assert!(!b_has_energy_from_levels(&b_levels, core_s, core_e));
        let v = derive_gap_equivalence(
            &a_levels,
            core_s,
            core_e,
            Some(&b_levels),
            Some((core_s, core_e)),
            &on(),
        );
        assert_eq!(v.class, SharedSilence, "{v:?}");
        assert_eq!(v.donor_silence_fraction, Some(1.0), "{v:?}");
        assert!(occupancy_agrees_with_donor_silence(
            false,
            v.donor_silence_fraction,
            0.5
        ));
    }
}
