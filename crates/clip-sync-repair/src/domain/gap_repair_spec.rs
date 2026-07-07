//! `GapRepairSpec` — the typed fix-list + repair-params handoff between **characterize** and **execute**
//! (pipeline redesign §2.5, step 6a). One spec per planned `FillRegion` after characterization; the executor
//! consumes specs and never re-decides. See `docs/TEMP-pipeline-perf-redesign-plan.md` §2.5.2 / §2.5.2a for
//! the full derivation.
//!
//! ## Layering
//! This is **domain** — pure data + pure classification. It deliberately does NOT define
//! `to_fingerprint_summary`: `GapFingerprint` is an *application*-layer export schema, so the
//! `GapRepairSpec -> GapFingerprint` projection lives in `application/` (step 8), not here (domain must not
//! depend on application). The characterize/execute logic that *builds* and *consumes* specs is likewise
//! application-layer (6b/6c); this module is only the wire type + the parts that are pure functions of it.
//!
//! ## Invariants
//! * **Single seam-scoring authority (A7 / §2.5.7 #2):** the executor and any export READ the seam
//!   correlations stored here; they never recompute. `SilenceSplice`'s seam fields are copied from the one
//!   `DualFitResult`; `Bracket`'s `seam_pre/seam_post` are the characterize-decided report-vs-splice
//!   reconciliation (§2.5.7 #4).
//! * **`cell()` is total** (§2.5.2): a projection of the finite verdict/reason space onto the vocabulary
//!   cells — no `Option`, no catch-all. `cell_for_skip_reason` is wildcard-free so a new
//!   [`GapPatchSkipReason`] variant is a compile error until it is classified (C4b).

use crate::domain::donor::DonorInterior;
use crate::domain::gap_fill::GapFillSkipped;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::patch_result::GapPatchSkipReason;
use crate::domain::policies::{FillAlignment, RefinedGapFrames, SeamResidualVerdict};

/// [gap-vocabulary.md] cell — a gap-type we **see and reconcile to an action** (patch OR a *reasoned* skip).
/// Derived from the real code disposition, not an abstract axis space; `cell()` is total onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapRepairCell {
    /// gate `Ok` → `RegionPatch` — action: patch.
    BracketPatch,
    /// dual-fit accepted → `RegionPatch` — action: dual-fit patch.
    SilenceSplice,
    /// `GapPatchSkipReason::ProgramQuiet` (nominal ∨ aligned-donor-dead) — action: skip, nothing to fill.
    ProgramQuiet,
    /// `StructureAlignmentFailed` → `BoundaryAlignmentFailed` — action: skip, no placement found.
    NoPlacement,
    /// Bare `CorrelationBelowThreshold`: bracket-exhausted, donor occupied, seams don't recover — B has
    /// different content (§4.6 decorrelated regime; zero re-anchor members). Action: reasoned skip.
    Decorrelated,
    /// `ResidualHeadroomExceeded`: seams pass the waveform gate but residual cancellation shows B ≠ A
    /// (echo/repeat/similar-but-different). Action: reasoned skip (false same-source).
    ResidualVeto,
    /// Unfillable family — structurally cannot fill: `BExtractFailed` / `AlignedSegmentOutOfRange` /
    /// `ZeroLengthGap`. Action: definite skip (no judgment). Plan-time Tail/coverage is the same family's
    /// plan-scope arm, carried on [`GapRepairPlan::skipped`], not per-region.
    Unfillable,
}

/// Where a D/R measurement was taken — part of the golden diff key (§4.3): a refactor that moves a field
/// across placements must fail the harness even if the value looks plausible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Gross `b_mapped`, 1 s window — `baseline_lag` / `splice` / `peak_z`.
    GrossBMapped,
    /// Seam-local, 250 ms ± `SEAM_LOCAL_REFINE_MS`, nominal-anchored — `splice_dualfit`.
    SeamLocal,
    /// Nominal `b_mapped .. + gap_frames` span — `donor_interior_nominal`.
    NominalSpan,
    /// Aligned bridge span — `donor_interior`.
    AlignedBridge,
    /// Structure-slid throat (zero-move) — gate decision / residual / bracket counts.
    GateThroat,
    /// A's own timeline — `levels` floors.
    ASide,
}

// ---------------------------------------------------------------------------------------------------------
// D/R payload (§2.5.2a) — typed 1:1 against the golden `GapRow`; the fingerprint export is a projection.
// ---------------------------------------------------------------------------------------------------------

/// Placement = [`Placement::GrossBMapped`]. From `baseline_lag` mono pre/post + `SpliceSummary`.
#[derive(Debug, Clone, Default)]
pub struct RegistrationTags {
    pub pre_peak_r: Option<f64>,
    pub post_peak_r: Option<f64>,
    pub pre_frac_lag_ms: Option<f64>,
    pub post_frac_lag_ms: Option<f64>,
    pub pre_peak_z: Option<f64>,
    pub post_peak_z: Option<f64>,
    pub pre_prominence: Option<f64>,
    pub post_prominence: Option<f64>,
    pub step_ms: Option<f64>,
    pub edge_pinned: Option<bool>,
}

/// Placement = [`Placement::SeamLocal`]. From `splice_dualfit` + `DualFitResult` lags. `pre_lag`/`post_lag`
/// make the per-shoulder B placement reconstructable (A7). Uniqueness validators are `None` on the
/// production characterize path (Tier-3; §2.5.2a decision 1).
#[derive(Debug, Clone)]
pub struct SeamLocalTags {
    pub pre_seam_r: f64,
    pub post_seam_r: f64,
    pub post_seam_global_r: f64,
    pub trim_frames: i64,
    pub gate_pass: bool,
    pub pre_lag: i64,
    pub post_lag: i64,
    pub pre_seam_prom: Option<f64>,
    pub post_seam_prom: Option<f64>,
    pub pre_seam_z: Option<f64>,
    pub post_seam_z: Option<f64>,
}

/// Placement = [`Placement::GateThroat`].
#[derive(Debug, Clone, Default)]
pub struct GateTags {
    pub brackets_total: usize,
    pub brackets_passing: usize,
    pub closest_failure_stage: Option<String>,
    pub structure_min: Option<f64>,
    pub seam_min: Option<f64>,
    pub best_bracket_seam: Option<f64>,
    pub residual: Option<SeamResidualVerdict>,
}

/// Placement = [`Placement::ASide`]. From `levels` (`LevelProfile`).
#[derive(Debug, Clone, Copy)]
pub struct LevelTags {
    pub a_gap_floor_db: f64,
    pub a_noise_floor_db: f64,
}

/// The decision/repair-bearing coordinates for one characterized gap — populated for EVERY gap regardless of
/// verdict. The single source the fingerprint export + golden record project from. No X-set here.
#[derive(Debug, Clone)]
pub struct GapRepairTags {
    pub registration: RegistrationTags,
    /// `Some` iff dual-fit detect ran (bracket-exhausted skip path).
    pub seam_local: Option<SeamLocalTags>,
    /// Placement = [`Placement::NominalSpan`].
    pub donor_nominal: Option<DonorInterior>,
    /// Placement = [`Placement::AlignedBridge`].
    pub donor_aligned: Option<DonorInterior>,
    pub gate: GateTags,
    pub levels: LevelTags,
}

// ---------------------------------------------------------------------------------------------------------
// Geometry + verdict + strategy
// ---------------------------------------------------------------------------------------------------------

/// B haystack window within `b_samples_full` (interleaved frame indices).
#[derive(Debug, Clone, Copy)]
pub struct BExtractWindow {
    pub start_frame: usize,
    pub end_frame: usize,
    /// Nominal gap start within the extract window — the anchor for seam-local placement reconstruction.
    pub b_mapped_start_frame: usize,
}

/// Committed repair parameters the executor consumes. `Bracket` carries indices (executor slices the fill);
/// `SilenceSplice` carries synthesized PCM (not reconstructable from indices — the PCM-ownership asymmetry).
#[derive(Debug, Clone)]
pub enum GapRepairStrategy {
    Bracket {
        /// Single rigid B placement — start_frame, fill_frames, report pre/post r.
        alignment: FillAlignment,
        structure_start_frame: usize,
        structure_trusted: bool,
        anchor_seam_used: bool,
        anchor_bracket_move_frames: usize,
        anchor_trusted: bool,
        /// Report-vs-splice reconciliation decided AT CHARACTERIZE (§2.5.7 #4). Final chosen seam
        /// correlations; `used_splice` records which placement won (false = gate-throat report, true =
        /// assembled-fill splice). The executor re-slices the identical fill and READS these — never re-scores.
        seam_pre: f64,
        seam_post: f64,
        used_splice: bool,
        /// The RECONCILED confidence tier.
        confidence: FillConfidence,
        gap_start_adjust_frames: i64,
        gap_end_adjust_frames: i64,
        fit_used_boundary_grid: bool,
        fit_boundary_grid_cells: Option<u32>,
        residual: Option<SeamResidualVerdict>,
        /// 1.0, or the RMS gain computed at characterize time (needs the sliced B).
        normalize_gain: f32,
    },
    SilenceSplice {
        /// Filled, length-reconciled interleaved PCM (`gap_frames × channels`).
        fill: Vec<f32>,
        /// Copied from the one `DualFitResult` (single-source, A7) — same values as `tags.seam_local`.
        pre_seam_r: f64,
        post_seam_r: f64,
        /// Per-shoulder seam-local lags (frames), re-anchored on nominal `b_mapped`. Required to rebuild
        /// `b_pre_seam` / `b_post_seam` for A6 residual and A7 re-validation.
        pre_lag: i64,
        post_lag: i64,
        /// `bridge − gap`.
        trim_frames: i64,
        /// Per-shoulder residual (A6 gate).
        residual: Option<SeamResidualVerdict>,
        confidence: FillConfidence,
    },
}

/// Patch (a strategy) or a reasoned/mechanical skip. `Skip` carries the axis-derived [`GapRepairCell`]
/// (computed by characterize) plus the exact `GapPatchSkipReason` (source truth), so nothing the code emits
/// is lost. Dual-fit eligibility is derivable from `reason` (`BoundaryAlignmentFailed` ⟺ the gate's
/// `StructureAlignmentFailed`), so no application-layer `SeamGateFailure` dependency is needed here.
#[derive(Debug, Clone)]
pub enum GapRepairVerdict {
    Patch(GapRepairStrategy),
    Skip {
        cell: GapRepairCell,
        reason: GapPatchSkipReason,
    },
}

/// One entry per `FillRegion` after characterization (fix-list + repair-params).
#[derive(Debug, Clone)]
pub struct GapRepairSpec {
    /// Index into `GapReport.gaps` (stable within one scan recipe).
    pub gap_index: usize,
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    /// `resolve_gap_offset_secs` result for this gap.
    pub gap_offset_secs: f64,
    /// Refined throat on A's PCM timeline.
    pub refined: RefinedGapFrames,
    pub b_extract: BExtractWindow,
    pub crossfade_secs: f64,
    pub verdict: GapRepairVerdict,
    /// Full D/R payload — populated for every gap regardless of verdict (§2.5.2a).
    pub tags_ctx: GapRepairTags,
}

impl GapRepairSpec {
    /// The gap's cell identity — a total projection of the verdict (§2.5.2). Tier-independent: a
    /// Silence-splice keeps its cell whether it was patched or (dual-fit off) skipped as
    /// `Skip { cell: SilenceSplice, .. }`.
    pub fn cell(&self) -> GapRepairCell {
        match &self.verdict {
            GapRepairVerdict::Patch(GapRepairStrategy::Bracket { .. }) => GapRepairCell::BracketPatch,
            GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice { .. }) => GapRepairCell::SilenceSplice,
            GapRepairVerdict::Skip { cell, .. } => *cell,
        }
    }
}

/// Fix-list for one write run.
#[derive(Debug, Clone)]
pub struct GapRepairPlan {
    pub specs: Vec<GapRepairSpec>,
    /// Plan-time exclusions (Tail / coverage / track) — the Unfillable family's plan-scope arm.
    pub skipped: Vec<GapFillSkipped>,
}

/// Fallback cell for a bare skip reason — used by characterize *before* sub-classification layers a richer
/// cell on top (a `CorrelationBelowThreshold` becomes `SilenceSplice`/`ProgramQuiet` when the donor/seam
/// axes say so; this returns its un-sub-classified default, `Decorrelated`).
///
/// **Wildcard-free by design (C4b):** a new [`GapPatchSkipReason`] variant makes this fail to compile until
/// it is assigned a cell — the compile-time guarantee that no source disposition escapes the vocabulary.
pub fn cell_for_skip_reason(reason: &GapPatchSkipReason) -> GapRepairCell {
    match reason {
        GapPatchSkipReason::BoundaryAlignmentFailed => GapRepairCell::NoPlacement,
        GapPatchSkipReason::ProgramQuiet => GapRepairCell::ProgramQuiet,
        GapPatchSkipReason::CorrelationBelowThreshold { .. } => GapRepairCell::Decorrelated,
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => GapRepairCell::ResidualVeto,
        GapPatchSkipReason::BExtractFailed
        | GapPatchSkipReason::AlignedSegmentOutOfRange
        | GapPatchSkipReason::ZeroLengthGap => GapRepairCell::Unfillable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C4b backstop (the primary guarantee is the wildcard-free match above): every `GapPatchSkipReason`
    /// maps to a defined cell. If a variant is added to `patch_result.rs`, `cell_for_skip_reason` stops
    /// compiling; if the mapping is wrong, this fails.
    #[test]
    fn cell_for_skip_reason_is_exhaustive_and_correct() {
        let corr = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.0,
            post_correlation: 0.0,
            min_correlation: 0.0,
            best_attempt: None,
        };
        let residual = GapPatchSkipReason::ResidualHeadroomExceeded {
            pre_correlation: 0.0,
            post_correlation: 0.0,
            headroom_db: 0.0,
            floor_pre_db: 0.0,
            floor_post_db: 0.0,
            margin_db: 0.0,
        };
        let cases = [
            (GapPatchSkipReason::BoundaryAlignmentFailed, GapRepairCell::NoPlacement),
            (GapPatchSkipReason::ProgramQuiet, GapRepairCell::ProgramQuiet),
            (corr, GapRepairCell::Decorrelated),
            (residual, GapRepairCell::ResidualVeto),
            (GapPatchSkipReason::BExtractFailed, GapRepairCell::Unfillable),
            (GapPatchSkipReason::AlignedSegmentOutOfRange, GapRepairCell::Unfillable),
            (GapPatchSkipReason::ZeroLengthGap, GapRepairCell::Unfillable),
        ];
        for (reason, want) in cases {
            assert_eq!(cell_for_skip_reason(&reason), want, "reason {reason:?}");
        }
    }

    /// `cell()` is a total projection of the verdict; a skip carries its axis-derived cell verbatim.
    #[test]
    fn cell_reads_the_stored_verdict_cell() {
        let spec = |verdict| GapRepairSpec {
            gap_index: 0,
            a_start_secs: 0.0,
            a_end_secs: 1.0,
            gap_offset_secs: 0.0,
            refined: RefinedGapFrames { start_frame: 0, end_frame: 48_000 },
            b_extract: BExtractWindow { start_frame: 0, end_frame: 96_000, b_mapped_start_frame: 24_000 },
            crossfade_secs: 0.01,
            verdict,
            tags_ctx: GapRepairTags {
                registration: RegistrationTags::default(),
                seam_local: None,
                donor_nominal: None,
                donor_aligned: None,
                gate: GateTags::default(),
                levels: LevelTags { a_gap_floor_db: -70.0, a_noise_floor_db: -60.0 },
            },
        };
        let decorrelated = spec(GapRepairVerdict::Skip {
            cell: GapRepairCell::Decorrelated,
            reason: GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                min_correlation: 0.0,
                best_attempt: None,
            },
        });
        assert_eq!(decorrelated.cell(), GapRepairCell::Decorrelated);
    }
}
