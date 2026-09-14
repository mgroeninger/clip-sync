use std::path::PathBuf;

use serde::Serialize;

use crate::domain::{AudioTimelineSkew, ScanAlignment};

use crate::domain::gap_equivalence::GapEquivalenceVerdict;
use crate::domain::track_match::TrackCompatibility;

/// Diagnostic comparison of the Chromaprint alignment offset vs the silence-structure-derived
/// offset produced by the R3 bidirectional scan cross-check.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapOffsetAgreement {
    pub silence_based_offset_secs: f64,
    pub alignment_offset_secs: f64,
    /// Absolute difference between the two estimates.
    pub delta_secs: f64,
    /// `true` when `delta_secs` is within the configured tolerance.
    pub agrees: bool,
}

/// A silent region detected in video A's timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    /// Corresponding position in video B, mapped via `recommended_offset_secs`.
    /// `None` when alignment failed or produced no offset — B cannot be probed.
    pub video_b_start_secs: Option<f64>,
    pub video_b_end_secs: Option<f64>,
    /// Whether video B has audio energy at this position (potential fill source).
    pub b_has_energy: bool,
    /// Paired **B minus A** level offset over a collar of A blocks either side of this gap —
    /// the hole's own master relationship. Stamped at scan time, where both block-level timelines
    /// and the global offset are live; see
    /// [`collar_level_offset`](crate::domain::program_levels::collar_level_offset).
    ///
    /// `None` covers every reason at once — B not scanned, no alignment offset, channel counts not
    /// comparable, or nothing in the collar paired — so a consumer must never infer *which* from
    /// the absence. It does **not** mean "too few pairs": a thin collar is reported, and
    /// [`MIN_LOCAL_PAIRS`](crate::domain::program_levels::MIN_LOCAL_PAIRS) is applied by whoever
    /// turns one into a gain.
    ///
    /// Measurement only: nothing applies a gain from it. Plan:
    /// `docs/dev/archive/TEMP-local-paired-gain-plan.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collar_level: Option<crate::domain::program_levels::CollarLevelOffset>,
}

impl Gap {
    pub fn duration_secs(&self) -> f64 {
        self.video_a_end_secs - self.video_a_start_secs
    }

    pub fn is_fillable(&self) -> bool {
        self.video_b_start_secs.is_some() && self.b_has_energy
    }

    /// The gap's mapped B span, or `None` when it is absent, degenerate, or negative.
    ///
    /// The single guarded way to read B coordinates off a gap. Rejects the same shapes
    /// [`b_range_fully_scanned`](crate::domain::cross_check::b_range_fully_scanned) rejects —
    /// the two predicates must not drift. In particular a half-mapped gap (start but no end)
    /// is `None` rather than a start on B's timeline paired with an end on A's, and a negative
    /// mapped start is `None` rather than something a caller clamps in one expression and
    /// reports unclamped in the next.
    pub fn mapped_b_span(&self) -> Option<(f64, f64)> {
        match (self.video_b_start_secs, self.video_b_end_secs) {
            (Some(start), Some(end)) if start < end && start >= 0.0 => Some((start, end)),
            _ => None,
        }
    }

    /// Operator-facing reason when [`is_fillable`](Self::is_fillable) is false.
    pub fn unfillable_label(&self) -> &'static str {
        if self.video_b_start_secs.is_some() {
            // Mapped donor is silent — shared pause / nothing to copy (not a missing alignment).
            "both sides silent"
        } else {
            "unmapped"
        }
    }
}

/// True when `[start_secs, end_secs]` lies entirely inside a timeline window.
///
/// Used by fill planning and silence cross-check gating.
pub fn interval_fully_within_window(
    start_secs: f64,
    end_secs: f64,
    window_start_secs: f64,
    window_end_secs: f64,
) -> bool {
    start_secs >= window_start_secs && end_secs <= window_end_secs
}

/// Default `min_gap_ms`. Sensitive scan default (2026-07-20): catches sub-second dropouts ffmpeg
/// `silencedetect` sees. The scan-time equivalence gate (`skip_equivalent_gaps`) drops the
/// mutual/ambient-silence extras this surfaces, so "find everything, patch only real dropouts" is
/// the default pairing.
pub const DEFAULT_MIN_GAP_MS: u64 = 500;

/// Default `silence_peak_fraction`.
pub const DEFAULT_SILENCE_PEAK_FRACTION: f32 = 0.01;

/// Default `scan_block_ms`. 100 ms analysis blocks (sensitive default) — finer silence-run
/// resolution and the equivalence gate's measurement granularity. Media-validated identical to the
/// fingerprint diagnostic path (8 pairs, 121 gaps) — but see
/// `docs/dev/archive/TEMP-equivalence-divergence-findings.md` F15: a divergent gap was found
/// 2026-07-30, and the two paths differ by more than granularity, so "identical" holds for that
/// corpus, not as an invariant. Since I1 this value is *also* the diagnostic path's bin width.
pub const DEFAULT_SCAN_BLOCK_MS: u64 = 100;

/// Default **configured** silence hold. Quantizes to
/// [`DEFAULT_SILENCE_HOLD_BLOCKS`] at [`DEFAULT_SCAN_BLOCK_MS`]; the two are kept together so the
/// relationship the scanner enforces is visible at the definition rather than inferred from two
/// unrelated literals.
pub const DEFAULT_SILENCE_HOLD_MS: u64 = 500;

/// Default hold expressed in blocks — `DEFAULT_SILENCE_HOLD_MS / DEFAULT_SCAN_BLOCK_MS`, the form
/// [`ScanRecipe::with_hold_blocks`] takes. Exact by construction; the assertion in
/// [`ScanRecipe::silence_hold_blocks`] holds for the default recipe.
pub const DEFAULT_SILENCE_HOLD_BLOCKS: u32 =
    (DEFAULT_SILENCE_HOLD_MS / DEFAULT_SCAN_BLOCK_MS) as u32;

/// Default `absolute_silence_rms` as **normalized** amplitude (33 i16 units on the operator-facing
/// 0–32767 scale). See `normalize_absolute_silence_rms_i16`.
pub const DEFAULT_ABSOLUTE_SILENCE_RMS: f32 = 33.0 / 32767.0;

/// Default `decode_chunk_secs`. Decode throughput only — deliberately **not** part of
/// [`ScanRecipe`], because it cannot move a gap boundary.
pub const DEFAULT_DECODE_CHUNK_SECS: u64 = 10;

/// The scan knobs that determine **which gaps are detected**. Equality means "same gap list".
///
/// `PartialEq` is **bitwise** (including the two `f32` fields) and intentional: values are copied
/// from config / request construction, never recomputed. Do not replace with an epsilon comparison.
/// `decode_chunk_secs` is deliberately excluded — decode throughput only; it cannot move a gap
/// boundary.
///
/// Fields are private so effective hold cannot drift from `blocks × scan_block_ms`. Construct with
/// [`Self::with_hold_blocks`] (the only write path for hold geometry).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanRecipe {
    min_gap_ms: u64,
    /// **Effective** hold — `silence_hold_blocks * scan_block_ms` (what the scan applied after block
    /// quantization), not the configured `RepairConfig::silence_hold_ms`.
    silence_hold_ms: u64,
    scan_block_ms: u64,
    silence_peak_fraction: f32,
    /// Normalized amplitude in `[0, 1)` (default ≈ `0.001007`). Operator-facing 0–32767 i16 units
    /// are CLI-only via `normalize_absolute_silence_rms_i16`.
    absolute_silence_rms: f32,
}

impl ScanRecipe {
    /// Build a recipe from the blocks the scanner will apply. Sets
    /// [`Self::silence_hold_ms`] = `silence_hold_blocks × scan_block_ms` — the only place that
    /// multiplication should live.
    pub fn with_hold_blocks(
        min_gap_ms: u64,
        silence_hold_blocks: u32,
        scan_block_ms: u64,
        silence_peak_fraction: f32,
        absolute_silence_rms: f32,
    ) -> Self {
        Self {
            min_gap_ms,
            silence_hold_ms: u64::from(silence_hold_blocks) * scan_block_ms,
            scan_block_ms,
            silence_peak_fraction,
            absolute_silence_rms,
        }
    }

    /// Replace the absolute RMS floor (normalized amplitude). Hold geometry is unchanged.
    #[must_use]
    pub fn with_absolute_silence_rms(mut self, absolute_silence_rms: f32) -> Self {
        self.absolute_silence_rms = absolute_silence_rms;
        self
    }

    pub fn min_gap_ms(self) -> u64 {
        self.min_gap_ms
    }

    pub fn silence_hold_ms(self) -> u64 {
        self.silence_hold_ms
    }

    pub fn scan_block_ms(self) -> u64 {
        self.scan_block_ms
    }

    pub fn silence_peak_fraction(self) -> f32 {
        self.silence_peak_fraction
    }

    pub fn absolute_silence_rms(self) -> f32 {
        self.absolute_silence_rms
    }

    /// Blocks implied by the effective hold (`silence_hold_ms / scan_block_ms`). Exact when the
    /// recipe was built via [`Self::with_hold_blocks`].
    pub fn silence_hold_blocks(self) -> u32 {
        if self.scan_block_ms == 0 {
            return 0;
        }
        debug_assert_eq!(
            self.silence_hold_ms % self.scan_block_ms,
            0,
            "silence_hold_ms must be a multiple of scan_block_ms (build via with_hold_blocks)"
        );
        (self.silence_hold_ms / self.scan_block_ms) as u32
    }

    pub fn scan_block_secs(self) -> f64 {
        self.scan_block_ms as f64 / 1000.0
    }

    pub fn min_gap_secs(self) -> f64 {
        self.min_gap_ms as f64 / 1000.0
    }
}

/// The production scan defaults, built through [`Self::with_hold_blocks`] so hold geometry is
/// exact rather than a zeroed placeholder.
///
/// **Deliberately the production recipe, not the recipe most tests spell out by hand.** A recipe
/// is a claim about which gaps were detected, so the default has to be a set of values that
/// actually detects gaps; a zeroed one would report 0 hold blocks, which is silently meaningless
/// rather than loud. Existing call sites that name their own values keep them — this exists so a
/// fixture that does not care about scan knobs can write `..Default::default()` instead of
/// restating five.
impl Default for ScanRecipe {
    fn default() -> Self {
        Self::with_hold_blocks(
            DEFAULT_MIN_GAP_MS,
            DEFAULT_SILENCE_HOLD_BLOCKS,
            DEFAULT_SCAN_BLOCK_MS,
            DEFAULT_SILENCE_PEAK_FRACTION,
            DEFAULT_ABSOLUTE_SILENCE_RMS,
        )
    }
}

/// Full gap scan report produced by `ScanGaps`.
#[derive(Debug, Clone)]
pub struct GapReport {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    /// Audio track comparison (channels/rate). `None` when B could not be opened or has no
    /// decodable track — the scan still reports A's gaps.
    pub track_compatibility: Option<TrackCompatibility>,
    pub alignment: ScanAlignment,
    pub gaps: Vec<Gap>,
    /// Per-gap silence-character classification (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate), **index-parallel to
    /// `gaps`**. Always populated by the scan (additive/advisory); empty on reports built before the gate or
    /// by test/legacy constructors. Consumed by `build_gap_fill_plan` only when `skip_equivalent_gaps` is set.
    pub gap_equivalence: Vec<GapEquivalenceVerdict>,
    /// Present when `scan_both` was enabled and both A and B had silence intervals to compare.
    pub gap_offset_agreement: Option<GapOffsetAgreement>,
    /// Decode chunk size used during sequential scan (seconds).
    pub decode_chunk_secs: u64,
    /// Collar half-width (seconds either side of the hole) the per-gap
    /// [`Gap::collar_level`] readings in this report were measured over — the scan's
    /// `local_pair_collar_secs`, echoed so the report says what produced it.
    ///
    /// Outside [`Self::recipe`] for the same reason [`Self::decode_chunk_secs`] is: the recipe's
    /// contract is `PartialEq` ⇔ same gap list, and a collar width cannot move a gap boundary.
    ///
    /// `None` on legacy/test constructors only — a real scan always stamps it. The knob is
    /// config-only with no CLI flag, so without this the config that set it is the sole record,
    /// and that config normally lives beside the dumps in an ephemeral directory: two census arms
    /// differing only in collar width would be byte-indistinguishable in shape, and a differential
    /// between them attributable to nothing sturdier than a directory name.
    pub local_pair_collar_secs: Option<f64>,
    /// Scan knobs that determined which gaps were detected (`PartialEq` ⇔ same gap list).
    pub recipe: ScanRecipe,
    /// When query-reference alignment is used, only gaps inside the mapped clip coverage are fillable.
    pub limit_fill_to_mapped_region: bool,
    /// How far the B silence/level scan progressed on B's native timeline (seconds). `None` when B
    /// was not scanned. Gaps whose mapped core extends past this were not reviewed for donor
    /// occupancy (`b_has_energy` is fail-closed).
    pub b_scanned_end_secs: Option<f64>,
    /// `true` when the B walk aborted mid-file (decode/seek error). Continue is intentional
    /// (report-only safe); callers should surface the truncation timestamp so users know later
    /// mapped gaps were not reviewed.
    pub b_scan_truncated: bool,
    /// Maximum |PTS − sample-clock| observed during gap scan on video A, when measurable.
    pub audio_timeline_skew: Option<AudioTimelineSkew>,
    /// Whole-program A/B level readings derived from the scan's block-level timelines
    /// (`domain::program_levels`; plan `docs/dev/archive/TEMP-ab-program-levels-plan.md`). Capture only —
    /// **nothing in the patch path reads this**, and no gain is derived from it. `None` on
    /// legacy/test constructors and when A produced no level timeline.
    pub program_levels: Option<crate::domain::program_levels::ProgramLevels>,
    /// Scan-time B-stall warning (A5; `docs/dev/archive/TEMP-b-stall-warning-plan.md`). Advisory only —
    /// nothing in the plan or patch path reads it. A real scan always stamps it (so the floor is
    /// echoed); `None` on legacy/test constructors only.
    pub b_stall: Option<crate::domain::b_stall::BStallReport>,
}

/// An empty report carrying the production [`ScanRecipe`] and no gaps.
///
/// Exists so a fixture can pin one field with `..Default::default()` instead of restating every
/// other one. `video_a` / `video_b` are empty paths and `alignment` is the default: a default
/// report describes **no scan**, and the only honest stand-in for "which media" is nothing. Real
/// construction sites must keep setting them.
impl Default for GapReport {
    fn default() -> Self {
        Self {
            video_a: PathBuf::new(),
            video_b: PathBuf::new(),
            track_compatibility: None,
            alignment: ScanAlignment::default(),
            gaps: Vec::new(),
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: DEFAULT_DECODE_CHUNK_SECS,
            local_pair_collar_secs: None,
            recipe: ScanRecipe::default(),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
            b_stall: None,
        }
    }
}

impl GapReport {
    /// Gaps where B has mapped audio energy (ignores track layout and mapped-region gate).
    pub fn fillable_count(&self) -> usize {
        self.gaps.iter().filter(|g| g.is_fillable()).count()
    }

    /// Whether the selected A/B track layout allows any splice fill.
    pub fn patch_allowed(&self) -> bool {
        matches!(
            self.track_compatibility.as_ref().map(|tc| tc.verdict),
            Some(super::track_match::CompatibilityVerdict::Identical)
                | Some(super::track_match::CompatibilityVerdict::Compatible)
        )
    }

    /// Gaps that would be included in a fill plan (B energy + patch-allowed tracks + region gate).
    pub fn repairable_count(&self) -> usize {
        if !self.patch_allowed() {
            return 0;
        }
        self.gaps
            .iter()
            .filter(|g| self.is_gap_repairable(g))
            .count()
    }

    /// True when a gap is not **fully** covered by the query-reference mapped region on A.
    ///
    /// Full containment is required by design: B only has audio for the mapped region, so a gap
    /// that straddles a region boundary is only partly covered. Filling it would splice
    /// uncovered audio (silence / out-of-range B) into the exposed part, so such gaps are
    /// conservatively excluded rather than partially filled. Returns `false` (not outside) in
    /// symmetric mode or when no overlap was computed — the gate only applies to query mode.
    pub fn gap_outside_reference_coverage(&self, gap: &Gap) -> bool {
        if !self.alignment.query_reference_mode {
            return false;
        }
        let Some(overlap) = &self.alignment.start_overlap else {
            return false;
        };
        !interval_fully_within_window(
            gap.video_a_start_secs,
            gap.video_a_end_secs,
            overlap.video_a_start_secs,
            overlap.video_a_end_secs,
        )
    }

    /// The equivalence verdict for the gap at `index` (index-parallel to `gaps`), when the scan populated it.
    pub fn gap_equivalence_at(&self, index: usize) -> Option<&GapEquivalenceVerdict> {
        self.gap_equivalence.get(index)
    }

    fn is_gap_repairable(&self, gap: &Gap) -> bool {
        if !gap.is_fillable() {
            return false;
        }
        if self.limit_fill_to_mapped_region && self.gap_outside_reference_coverage(gap) {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap_with_b(start: Option<f64>, end: Option<f64>) -> Gap {
        Gap {
            video_a_start_secs: 10.0,
            video_a_end_secs: 20.0,
            video_b_start_secs: start,
            video_b_end_secs: end,
            b_has_energy: true,
            collar_level: None,
        }
    }

    /// The default recipe must be a recipe that could really have run: hold geometry exact, and
    /// every knob non-zero. A zeroed recipe reports 0 hold blocks, which is silently meaningless
    /// rather than loud — the reason `ScanRecipe` did not simply derive `Default`.
    #[test]
    fn default_recipe_is_a_valid_production_recipe() {
        let recipe = ScanRecipe::default();

        assert_eq!(recipe.min_gap_ms(), DEFAULT_MIN_GAP_MS);
        assert_eq!(recipe.scan_block_ms(), DEFAULT_SCAN_BLOCK_MS);
        assert_eq!(
            recipe.silence_peak_fraction(),
            DEFAULT_SILENCE_PEAK_FRACTION
        );
        assert_eq!(recipe.absolute_silence_rms(), DEFAULT_ABSOLUTE_SILENCE_RMS);

        // Effective hold is the quantized product, and it round-trips back to the block count.
        assert_eq!(recipe.silence_hold_ms(), DEFAULT_SILENCE_HOLD_MS);
        assert_eq!(recipe.silence_hold_blocks(), DEFAULT_SILENCE_HOLD_BLOCKS);
        assert_eq!(
            recipe.silence_hold_ms() % recipe.scan_block_ms(),
            0,
            "configured hold must quantize exactly at the default block size"
        );

        assert!(
            recipe.scan_block_ms() > 0,
            "a zeroed recipe detects nothing"
        );
        assert!(recipe.min_gap_ms() > 0);
    }

    /// The point of the impl: a fixture states the field it cares about and nothing else.
    #[test]
    fn default_report_lets_a_fixture_pin_one_field() {
        let report = GapReport {
            local_pair_collar_secs: Some(2.0),
            ..Default::default()
        };

        assert_eq!(report.local_pair_collar_secs, Some(2.0));
        assert_eq!(report.recipe, ScanRecipe::default());
        assert!(report.gaps.is_empty());
        assert_eq!(report.decode_chunk_secs, DEFAULT_DECODE_CHUNK_SECS);
    }

    #[test]
    fn mapped_b_span_accepts_a_well_formed_span() {
        assert_eq!(
            gap_with_b(Some(3.0), Some(13.0)).mapped_b_span(),
            Some((3.0, 13.0))
        );
        assert_eq!(
            gap_with_b(Some(0.0), Some(1.0)).mapped_b_span(),
            Some((0.0, 1.0))
        );
    }

    #[test]
    fn mapped_b_span_rejects_half_mapped_rather_than_mixing_timelines() {
        // The F10 shape: a B start with no B end previously fell back to the *A* end.
        assert_eq!(gap_with_b(Some(3.0), None).mapped_b_span(), None);
        assert_eq!(gap_with_b(None, Some(13.0)).mapped_b_span(), None);
        assert_eq!(gap_with_b(None, None).mapped_b_span(), None);
    }

    #[test]
    fn mapped_b_span_rejects_negative_and_degenerate_spans() {
        // A negative mapped start is rejected outright, not clamped in one expression and
        // reported unclamped in the next.
        assert_eq!(gap_with_b(Some(-0.5), Some(9.5)).mapped_b_span(), None);
        assert_eq!(gap_with_b(Some(3.0), Some(3.0)).mapped_b_span(), None);
        assert_eq!(gap_with_b(Some(13.0), Some(3.0)).mapped_b_span(), None);
    }

    #[test]
    fn mapped_b_span_matches_b_range_fully_scanned_on_shape() {
        // Same predicate shape, different job: this one has no coverage limit. Any span
        // `mapped_b_span` accepts must be shape-acceptable to the coverage check too.
        for (start, end) in [(3.0, 13.0), (0.0, 1.0)] {
            assert!(gap_with_b(Some(start), Some(end)).mapped_b_span().is_some());
            assert!(crate::domain::cross_check::b_range_fully_scanned(
                start,
                end,
                Some(end)
            ));
        }
        for (start, end) in [(-0.5, 9.5), (3.0, 3.0), (13.0, 3.0)] {
            assert!(gap_with_b(Some(start), Some(end)).mapped_b_span().is_none());
            assert!(!crate::domain::cross_check::b_range_fully_scanned(
                start,
                end,
                Some(end.max(start))
            ));
        }
    }
}
