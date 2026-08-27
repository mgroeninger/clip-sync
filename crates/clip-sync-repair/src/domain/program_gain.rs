//! Whole-program fill gain derived from the scan's paired A↔B offset.
//!
//! Plan: `docs/dev/TEMP-program-gain-plan.md` § 3–§ 4. This is the consumer
//! [`program_levels`](crate::domain::program_levels) was measured for; that module's doc line
//! "capture only — nothing here feeds a gain" stops being true at this module.
//!
//! # Two refusals, deliberately
//!
//! The question is *does B match A? if not, add the difference*, and on a matched pair it is exactly
//! that: [`AbLevelOffset::offset_db`] is the answer and it is applied as measured. Everything else
//! here defends one property — **the estimator returns a number even when the premise is false**. If
//! B is not a constant-gain copy of A but a differently-processed one, no scalar is the difference
//! and the median is a fiction.
//!
//! An earlier draft gated on four thresholds. Three of them defended against media the 51-pair
//! corpus has zero instances of, so the shipped policy is two:
//!
//! - [`ProgramGainParams::max_spread_db`] — the measurement's own confidence. Blocks that disagree
//!   mean B is not a gained copy of A.
//! - [`ProgramGainParams::max_program_gain_db`] — a **refusal**, not a clamp. Past it the offset is
//!   an implausible answer for two masters of the same program, and Tier 2 showed the one thing that
//!   produces such an answer is a dynamics mismatch spread cannot see (a 2:1-compressed B reads
//!   −8.387 dB at a spread of 1.838, inside the range of 45 of 51 corpus pairs). Clamping would
//!   apply 6 dB of a number there is reason to disbelieve; refusing applies none of it.
//!
//! **No deadband.** 0.5 dB was once refused as noise and is not noise: the offset is a median over
//! ~500 paired blocks, whose standard error is roughly `1.25 × MAD × 1.48 / √n` ≈ 0.055 dB on corpus
//! material. The corpus's ±0.26 dB readings are real, and applying them is both inaudible and right.
//!
//! **Chunk drift does not gate.** [`robust_chunk_drift_db`] is computed for *reporting* only —
//! demoted 2026-08-26 (plan § 4) because no corpus pair drifts past the threshold Tier 2
//! calibrated, it is undefined for any program under ~20 minutes, and its usable margin over the
//! corpus was 0.209 dB. Re-arm trigger: **any real pair measuring robust chunk drift above
//! 1.5 dB**. The residual until then is a 3.4–10 dB drift applying a wrong scalar, bounded by
//! `max_program_gain_db`.

use serde::Serialize;

use crate::domain::program_levels::{AbLevelOffset, AbLevelOffsetChunk, ProgramLevels};

/// Default `program_gain_max_spread_db`. Tier 2 (plan § 4b): the widest corpus pair whose offset
/// *is* a single scalar reads 2.412, and a 4:1-compressed B — where one is not — reads 2.770.
pub const DEFAULT_PROGRAM_GAIN_MAX_SPREAD_DB: f64 = 2.5;

/// Default `max_program_gain_db`. No corpus pair comes within 20× of it (max 0.260 dB), and it sits
/// under Tier 2's 8.387 dB phantom from a 2:1-compressed B. Deliberately **not** inherited from the
/// 12 dB local `max_fill_gain_db`, which bounds a different quantity with a different failure mode.
pub const DEFAULT_MAX_PROGRAM_GAIN_DB: f64 = 6.0;

/// Robust-chunk-drift reading above which the demoted drift gate should be reconsidered (plan § 4).
/// Nothing branches on this — it exists so the number in the report has a stated meaning.
///
/// **1.5, deliberately the same number as the gate Tier 2 calibrated** — restated from 1.0 on
/// 2026-08-27 (`docs/dev/settled.md` § C). 1.0 sat *below* the corpus maximum (pair 31 reads
/// 1.291), so it re-armed the latch against evidence already in the Tier 3 census: a trigger has to
/// name evidence we do not have. At 1.5 the review fires exactly when a pair appears that the gate
/// would have caught.
pub const CHUNK_DRIFT_REVIEW_DB: f64 = 1.5;

/// The two knobs. See the module docs for why there are two and not four.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramGainParams {
    pub max_spread_db: f64,
    pub max_program_gain_db: f64,
}

impl Default for ProgramGainParams {
    fn default() -> Self {
        Self {
            max_spread_db: DEFAULT_PROGRAM_GAIN_MAX_SPREAD_DB,
            max_program_gain_db: DEFAULT_MAX_PROGRAM_GAIN_DB,
        }
    }
}

/// Why a run applied no program gain. Four reasons, one per row of plan § 4 — plus the flag.
///
/// There is deliberately no `Deadband` and no `ChunkDrift` variant; both were dropped from the
/// design (module docs), and re-adding either needs the trigger recorded there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramGainRefusal {
    /// `program_gain = false` (`--no-program-gain`).
    FlagOff,
    /// No paired offset on the scan report: B was not scanned, or fewer than `MIN_PAIRS` blocks
    /// paired and the capture abstained rather than inventing one.
    NoOffset,
    /// A and B channel counts differ, so the offset carries a correlation-dependent constant that
    /// is not a gain (see [`crate::domain::program_levels`] module docs).
    ChannelsNotComparable,
    /// The paired blocks disagree: B is not a constant-gain copy of A, so no scalar is the answer.
    Spread,
    /// `|offset_db|` past `max_program_gain_db` — an implausible answer, and the known signature of
    /// a dynamics mismatch the spread check cannot see. Refused, **not** clamped.
    TooLarge,
}

impl ProgramGainRefusal {
    /// Stable short label for logs and the human summary line.
    pub fn label(self) -> &'static str {
        match self {
            Self::FlagOff => "flag-off",
            Self::NoOffset => "no-offset",
            Self::ChannelsNotComparable => "channels",
            Self::Spread => "spread",
            Self::TooLarge => "too-large",
        }
    }
}

/// What one run decided about program gain. Computed **once per run** (in
/// [`PatchRequestSettings::into_request`](crate::application::PatchRequestSettings::into_request)),
/// never per gap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgramGainDecision {
    /// Apply this linear multiplier to every B-derived fill.
    Applied {
        gain: f32,
        /// The measured `offset_db` this came from (`gain = 10^(-offset_db/20)`).
        offset_db: f64,
    },
    Refused {
        reason: ProgramGainRefusal,
    },
}

impl ProgramGainDecision {
    /// The multiplier to apply. `1.0` on every refusal — fail closed is unity, which is the
    /// pre-2026-08-26 behaviour of every fill path.
    pub fn gain(self) -> f32 {
        match self {
            Self::Applied { gain, .. } => gain,
            Self::Refused { .. } => 1.0,
        }
    }

    pub fn refusal(self) -> Option<ProgramGainRefusal> {
        match self {
            Self::Applied { .. } => None,
            Self::Refused { reason } => Some(reason),
        }
    }
}

/// The one decision function. `levels` is `report.program_levels`.
///
/// Order matters only for which reason is reported, not for the outcome: every failure is unity.
/// The flag is checked first so `--no-program-gain` reports as itself on a pair that would also have
/// failed a threshold.
pub fn program_fill_gain(
    levels: Option<&ProgramLevels>,
    enabled: bool,
    params: &ProgramGainParams,
) -> ProgramGainDecision {
    let refuse = |reason| ProgramGainDecision::Refused { reason };
    if !enabled {
        return refuse(ProgramGainRefusal::FlagOff);
    }
    // Preconditions: not thresholds. There is no number to apply.
    let Some(offset) = levels.and_then(|l| l.offset.as_ref()) else {
        return refuse(ProgramGainRefusal::NoOffset);
    };
    if !offset.channels_comparable {
        return refuse(ProgramGainRefusal::ChannelsNotComparable);
    }
    if !offset.offset_db.is_finite() || !offset.spread_db.is_finite() {
        return refuse(ProgramGainRefusal::NoOffset);
    }
    // Then the two refusals.
    if offset.spread_db > params.max_spread_db {
        return refuse(ProgramGainRefusal::Spread);
    }
    if offset.offset_db.abs() > params.max_program_gain_db {
        return refuse(ProgramGainRefusal::TooLarge);
    }
    // The fill is B content spliced into A, so the applied gain is the inverse of B−A.
    ProgramGainDecision::Applied {
        gain: 10f64.powf(-offset.offset_db / 20.0) as f32,
        offset_db: offset.offset_db,
    }
}

/// Range of per-chunk offsets over chunks holding at least half the median chunk population.
///
/// **Reported, never gated** (module docs). The population filter is not decoration: a raw
/// max-minus-min is a range statistic set by whichever chunk is least populated, which is why
/// `settled.md` § E records it failing corpus pair 24 on a single 359-block chunk. Tier 2 measures
/// 2.195 raw against 1.336 robust on the *same* fixture, so a threshold copied between the two
/// statistics is wrong by more than the corridor it sits in.
///
/// `None` when fewer than two chunks survive the filter — a range over one chunk says nothing
/// `offset_db` does not already say. Mirrors `scripts/measure/census-program-levels.ps1`.
pub fn robust_chunk_drift_db(chunks: &[AbLevelOffsetChunk]) -> Option<f64> {
    if chunks.len() < 2 {
        return None;
    }
    let mut populations: Vec<usize> = chunks.iter().map(|c| c.n_pairs).collect();
    populations.sort_unstable();
    let mid = populations.len() / 2;
    let median_pairs = if populations.len().is_multiple_of(2) {
        (populations[mid - 1] + populations[mid]) as f64 / 2.0
    } else {
        populations[mid] as f64
    };
    let floor = median_pairs / 2.0;
    let kept: Vec<f64> = chunks
        .iter()
        .filter(|c| c.n_pairs as f64 >= floor)
        .map(|c| c.offset_db)
        .collect();
    if kept.len() < 2 {
        return None;
    }
    let min = kept.iter().copied().fold(f64::INFINITY, f64::min);
    let max = kept.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some(max - min)
}

/// What the patch summary stamps once per run. Record-only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProgramGainReport {
    /// The multiplier applied to every B-derived fill (`1.0` when refused).
    pub gain: f32,
    /// The measured whole-program offset (B−A), when one existed — present even on a refusal that
    /// happened *after* the measurement, so a `too-large`/`spread` line names the number it refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread_db: Option<f64>,
    /// Robust chunk drift — **reported, not gated** (module docs). Above
    /// [`CHUNK_DRIFT_REVIEW_DB`] this run is the trigger to reconsider that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robust_drift_db: Option<f64>,
    /// `None` when the gain was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<ProgramGainRefusal>,
}

impl ProgramGainReport {
    pub fn new(decision: ProgramGainDecision, levels: Option<&ProgramLevels>) -> Self {
        let offset: Option<&AbLevelOffset> = levels.and_then(|l| l.offset.as_ref());
        Self {
            gain: decision.gain(),
            offset_db: offset.map(|o| o.offset_db),
            spread_db: offset.map(|o| o.spread_db),
            robust_drift_db: offset.and_then(|o| robust_chunk_drift_db(&o.chunks)),
            refused: decision.refusal(),
        }
    }

    /// True when the reported drift crosses the re-arm trigger in the module docs.
    pub fn drift_wants_review(&self) -> bool {
        self.robust_drift_db
            .is_some_and(|d| d > CHUNK_DRIFT_REVIEW_DB)
    }
}

/// The two halves of the applied fill gain, kept separable so a report can say which one did what.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackedFillGain {
    /// Whole-program A-to-B match for the run, or `1.0` on a refusal.
    pub program: f32,
    /// Hole-local residual boost toward the A-side border, or `1.0` when local normalize is off.
    pub local: f32,
    /// What the splice actually applies: `program * local`.
    pub total: f32,
}

/// Compose the two gains in the order the design requires (plan § 5).
///
/// The local half is a **residual**: it is measured on the fill *after* the program scalar has
/// already matched masters, which is why `b_fill_rms` is scaled before `compute_fill_gain` sees it.
/// Reading the raw B level here instead would correct the master offset twice — a `program^2` fill —
/// and dropping the program half on a normalizing run would leave the offset uncorrected wherever
/// the local boost clamps.
///
/// Scaling the RMS is the same thing as scaling the samples (`rms_interleaved` is homogeneous of
/// degree 1) and saves a PCM pass.
pub fn stack_fill_gain(
    program_gain: f32,
    a_border_rms: f32,
    b_fill_rms: f32,
    max_local_gain_db: f64,
    normalize_fill: bool,
) -> StackedFillGain {
    let local = if normalize_fill {
        crate::domain::policies::compute_fill_gain(
            a_border_rms,
            b_fill_rms * program_gain,
            max_local_gain_db,
        )
    } else {
        1.0
    };
    StackedFillGain {
        program: program_gain,
        local,
        total: program_gain * local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset(offset_db: f64, spread_db: f64) -> AbLevelOffset {
        AbLevelOffset {
            offset_db,
            spread_db,
            p10_db: offset_db,
            p90_db: offset_db,
            n_pairs: 500,
            n_active: 600,
            n_steady: 550,
            first_pair_secs: 0.0,
            last_pair_secs: 600.0,
            channels_comparable: true,
            chunks: Vec::new(),
        }
    }

    fn levels(offset: Option<AbLevelOffset>) -> ProgramLevels {
        use crate::domain::program_levels::ProgramLevelContext;
        let ctx = ProgramLevelContext {
            mid_db: Some(-20.0),
            floor_db: Some(-70.0),
            n_active: 600,
        };
        ProgramLevels {
            a: ctx,
            b: Some(ctx),
            channels_comparable: true,
            offset,
        }
    }

    fn decide(levels: &ProgramLevels) -> ProgramGainDecision {
        program_fill_gain(Some(levels), true, &ProgramGainParams::default())
    }

    #[test]
    fn polarity_is_the_inverse_of_b_minus_a() {
        // B 6 dB louder than A ⇒ the fill must come DOWN.
        let louder = decide(&levels(Some(offset(6.0, 0.5))));
        assert!(louder.gain() < 1.0, "{louder:?}");
        assert!((louder.gain() - 0.501_187).abs() < 1e-5, "{louder:?}");
        // B 6 dB quieter ⇒ boost.
        let quieter = decide(&levels(Some(offset(-6.0, 0.5))));
        assert!((quieter.gain() - 1.995_262).abs() < 1e-5, "{quieter:?}");
    }

    #[test]
    fn each_refusal_row_independently_yields_unity() {
        let params = ProgramGainParams::default();
        // Flag off.
        let off = program_fill_gain(Some(&levels(Some(offset(-3.0, 0.5)))), false, &params);
        assert_eq!(off.refusal(), Some(ProgramGainRefusal::FlagOff));
        assert_eq!(off.gain(), 1.0);
        // No offset — both shapes: no `program_levels` at all, and levels without an offset.
        assert_eq!(
            program_fill_gain(None, true, &params).refusal(),
            Some(ProgramGainRefusal::NoOffset)
        );
        assert_eq!(
            decide(&levels(None)).refusal(),
            Some(ProgramGainRefusal::NoOffset)
        );
        // Channels.
        let mut mismatched = offset(-3.0, 0.5);
        mismatched.channels_comparable = false;
        let channels = decide(&levels(Some(mismatched)));
        assert_eq!(
            channels.refusal(),
            Some(ProgramGainRefusal::ChannelsNotComparable)
        );
        assert_eq!(channels.gain(), 1.0);
        // Spread: 2.412 is the widest corpus pair and must pass; past 2.5 must not.
        assert!(decide(&levels(Some(offset(-3.0, 2.412))))
            .refusal()
            .is_none());
        let spread = decide(&levels(Some(offset(-3.0, 2.6))));
        assert_eq!(spread.refusal(), Some(ProgramGainRefusal::Spread));
        assert_eq!(spread.gain(), 1.0);
    }

    #[test]
    fn past_the_bound_refuses_rather_than_clamping() {
        // Tier 2's 2:1-compressed reading: a phantom offset at a spread no gate rejects.
        let compressed = decide(&levels(Some(offset(-8.387, 1.838))));
        assert_eq!(compressed.refusal(), Some(ProgramGainRefusal::TooLarge));
        assert_eq!(
            compressed.gain(),
            1.0,
            "clamping would apply 6 dB of a number we have reason to disbelieve"
        );
        // Exactly at the bound still applies — the refusal is strictly past it.
        assert!(decide(&levels(Some(offset(-6.0, 0.5)))).refusal().is_none());
    }

    #[test]
    fn small_real_offsets_apply_because_there_is_no_deadband() {
        // The largest offset on the 51-pair corpus. Well outside the ~0.055 dB standard error, so
        // it is a real bias and gets applied rather than snapping to unity.
        let decision = decide(&levels(Some(offset(0.26, 0.668))));
        assert!(decision.refusal().is_none(), "{decision:?}");
        assert!(
            (decision.gain() - 0.970_47).abs() < 1e-4,
            "expected a real sub-dB attenuation, got {decision:?}"
        );
    }

    #[test]
    fn chunk_drift_is_reported_but_never_gates() {
        let mut drifting = offset(-1.0, 0.75);
        drifting.chunks = vec![
            AbLevelOffsetChunk {
                start_secs: 0.0,
                end_secs: 600.0,
                offset_db: -1.0,
                n_pairs: 300,
            },
            AbLevelOffsetChunk {
                start_secs: 600.0,
                end_secs: 1200.0,
                offset_db: 4.342,
                n_pairs: 300,
            },
        ];
        let decision = decide(&levels(Some(drifting.clone())));
        assert!(
            decision.refusal().is_none(),
            "drift does not gate (plan § 4); it only reports"
        );
        let report = ProgramGainReport::new(decision, Some(&levels(Some(drifting))));
        let drift = report.robust_drift_db.expect("two populous chunks");
        assert!((drift - 5.342).abs() < 1e-6, "{report:?}");
        assert!(report.drift_wants_review(), "{report:?}");
    }

    /// The re-arm trigger must sit *above* the Tier 3 corpus maximum, or reading the census
    /// re-arms the latch on evidence already in hand — which is why 1.0 was retracted on
    /// 2026-08-27 (`docs/dev/settled.md` § C). Pair 31's 1.291 is that maximum.
    #[test]
    fn the_review_trigger_sits_above_the_corpus_maximum() {
        let corpus_max_robust_drift_db = 1.291;
        assert!(
            corpus_max_robust_drift_db < CHUNK_DRIFT_REVIEW_DB,
            "a trigger at or below {corpus_max_robust_drift_db} names evidence we already have"
        );
        let mut at_corpus_max = offset(-0.2, 0.668);
        at_corpus_max.chunks = vec![
            AbLevelOffsetChunk {
                start_secs: 0.0,
                end_secs: 600.0,
                offset_db: -0.2,
                n_pairs: 1500,
            },
            AbLevelOffsetChunk {
                start_secs: 600.0,
                end_secs: 1200.0,
                offset_db: -0.2 + corpus_max_robust_drift_db,
                n_pairs: 1500,
            },
        ];
        let levels = levels(Some(at_corpus_max));
        let report = ProgramGainReport::new(decide(&levels), Some(&levels));
        assert!(
            !report.drift_wants_review(),
            "no corpus pair may ask for review: {report:?}"
        );
    }

    #[test]
    fn robust_drift_drops_thin_chunks_where_a_raw_range_would_not() {
        // settled.md § E: a raw max-minus-min is set by whichever chunk is least populated.
        let chunks = vec![
            AbLevelOffsetChunk {
                start_secs: 0.0,
                end_secs: 600.0,
                offset_db: 0.10,
                n_pairs: 600,
            },
            AbLevelOffsetChunk {
                start_secs: 600.0,
                end_secs: 1200.0,
                offset_db: 0.15,
                n_pairs: 580,
            },
            AbLevelOffsetChunk {
                start_secs: 1200.0,
                end_secs: 1800.0,
                offset_db: 3.00,
                n_pairs: 25,
            },
        ];
        let robust = robust_chunk_drift_db(&chunks).expect("two chunks survive the filter");
        assert!((robust - 0.05).abs() < 1e-9, "{robust}");
        // One chunk (or none) says nothing `offset_db` does not already say.
        assert_eq!(robust_chunk_drift_db(&chunks[..1]), None);
        assert_eq!(robust_chunk_drift_db(&[]), None);
    }

    #[test]
    fn stacking_applies_the_program_offset_once_not_twice() {
        // A at 0.2 RMS, B 6 dB hot at 0.4: the program scalar alone already matches them.
        let g_program = 10f32.powf(-6.0 / 20.0);
        let stacked = stack_fill_gain(g_program, 0.2, 0.4, 12.0, true);
        assert!(
            (stacked.local - 1.0).abs() < 1e-3,
            "local must read a matched fill as needing nothing: {stacked:?}"
        );
        assert!(
            (stacked.total - g_program).abs() < 1e-3,
            "total must be the program gain, not its square ({}): {stacked:?}",
            g_program * g_program
        );
    }

    #[test]
    fn stacking_with_local_off_is_the_program_gain_alone() {
        let g_program = 10f32.powf(-6.0 / 20.0);
        // A border far above the fill, so a normalizing run would boost hard; with local off the
        // fill still lands at A's *master*, which is the point of defaulting normalize_fill off.
        let stacked = stack_fill_gain(g_program, 0.5, 0.05, 12.0, false);
        assert_eq!(stacked.local, 1.0);
        assert_eq!(stacked.total, g_program);
    }

    #[test]
    fn local_stays_boost_only_and_cannot_undo_a_program_attenuation() {
        // B 6 dB hot AND a quiet A border: local must not attenuate further, so the fill can never
        // end up below the program-matched level by way of the local half.
        let g_program = 10f32.powf(-6.0 / 20.0);
        let stacked = stack_fill_gain(g_program, 0.01, 0.4, 12.0, true);
        assert_eq!(stacked.local, 1.0, "{stacked:?}");
        assert_eq!(stacked.total, g_program, "{stacked:?}");
    }
}
