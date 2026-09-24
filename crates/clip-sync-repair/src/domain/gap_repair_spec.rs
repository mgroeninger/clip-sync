//! `GapRepairSpec` — the typed fix-list + repair-params handoff between **characterize** and **execute**
//! (pipeline redesign §2.5, step 6a). One spec per planned `FillRegion` after characterization; the executor
//! consumes specs and never re-decides. See `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §2.5.2 / §2.5.2a for
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
use crate::domain::gap_fill_fit::{FillConfidence, UnifiedFitBreakdown};
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
    /// Gross `b_mapped`, 1 s window — `lag_decision` / `splice` / `peak_z`.
    GrossBMapped,
    /// Seam-local, 250 ms ± `SEAM_LOCAL_REFINE_MS`, nominal-anchored — `splice_dualfit`.
    SeamLocal,
    /// Nominal `b_mapped .. + gap_frames` span — `donor_interior_nominal`.
    NominalSpan,
    /// Aligned bridge span — `donor_interior_aligned`.
    AlignedBridge,
    /// Structure-slid throat (zero-move) — gate decision / residual / bracket counts.
    GateThroat,
    /// A's own timeline — `levels` floors.
    ASide,
}

// ---------------------------------------------------------------------------------------------------------
// D/R payload (§2.5.2a) — typed 1:1 against the golden `GapRow`; the fingerprint export is a projection.
// ---------------------------------------------------------------------------------------------------------

/// Placement = [`Placement::GrossBMapped`]. From `lag_decision` mono pre/post + `SpliceSummary`.
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
    /// GCC-PHAT experiment (`TEMP-phat-seam-scoring-plan.md`), diagnostic-only: `None` on every real
    /// patch (production `dual_fit.rs` never computes this), `Some` only on the live fingerprint
    /// scan's own oracle measurement — so a from-decode replay of an actual patch naturally reads
    /// `None` here without needing a separate override.
    pub pre_seam_phat_r: Option<f64>,
    pub post_seam_phat_r: Option<f64>,
    pub pre_seam_phat_prom: Option<f64>,
    pub post_seam_phat_prom: Option<f64>,
    pub pre_seam_phat_z: Option<f64>,
    pub post_seam_phat_z: Option<f64>,
    /// Same-lag-agreement companions to the Pearson/PHAT triplets above (`TEMP-phat-seam-scoring-plan.md`):
    /// each curve's own fractional peak lag (ms), not `pre_lag`/`post_lag` — those are frames relative to
    /// the *nominal* seam center, while these are relative to the already-resolved seam placement, matching
    /// the window the prominence/PHAT curves were actually swept over. `None` under the same diagnostic-only
    /// rule as the PHAT fields.
    pub pre_seam_lag_ms: Option<f64>,
    pub post_seam_lag_ms: Option<f64>,
    pub pre_seam_phat_lag_ms: Option<f64>,
    pub post_seam_phat_lag_ms: Option<f64>,
    /// **Search provenance** for the two locks, carried because the fingerprint's `splice_dualfit` is
    /// rebuilt *from these tags* — anything not stored here never reaches the wire, however carefully
    /// the probe measured it. `None` means "this writer did not record it", never `false`: on a dump
    /// whose bridge is non-positive, `edge_pinned` is the difference between a real abut and a search
    /// that ran out of lags at the boundary, and reading an absent tag as "not pinned" would settle
    /// that question the wrong way.
    pub pre_seam_rescued: Option<bool>,
    pub post_seam_rescued: Option<bool>,
    pub pre_seam_edge_pinned: Option<bool>,
    pub post_seam_edge_pinned: Option<bool>,
    /// The non-positive bridge exists only because both seams clamped at B frame 0
    /// ([`crate::domain::seam_locks::SeamLocks::clamped`]).
    pub seams_clamped: Option<bool>,
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
/// `Default` = the all-`None`/default "partial" tags used in 6b (production doesn't compute the full D/R set
/// until step 8); both spec-build sites use `GapRepairTags::default()`.
#[derive(Debug, Clone, Default)]
pub struct GapRepairTags {
    pub registration: RegistrationTags,
    /// `Some` iff dual-fit detect ran (bracket-exhausted skip path).
    pub seam_local: Option<SeamLocalTags>,
    /// Placement = [`Placement::NominalSpan`].
    pub donor_nominal: Option<DonorInterior>,
    /// Placement = [`Placement::AlignedBridge`].
    pub donor_aligned: Option<DonorInterior>,
    pub gate: GateTags,
    /// `None` when A-levels weren't measured (early mechanical skip). Matches `GapRow.a_gap_floor_db`
    /// being `Option<f64>` — a non-optional `LevelTags` could not round-trip that.
    pub levels: Option<LevelTags>,
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
        /// Gap length (frames) the fill's correlate/seam-gate/border windows were sized from — the gap as
        /// it stood **before** the seam gate ran.
        ///
        /// Carried explicitly because it is **not** recoverable from `refined`. The gate can move the gap
        /// boundaries after the windows are sized (gate-mode seam-extension retry, fit-mode boundary
        /// search) while the windows stay fixed at their original size, and the extension retry's delta is
        /// not recorded in `gap_start_adjust_frames`/`gap_end_adjust_frames` — it re-runs the gate on the
        /// already-extended gap, so those record only the within-gate move. Re-deriving the windows from
        /// `refined` would therefore silently change the assembled fill on every extended gap.
        ///
        /// The *post*-gate length the fill is assembled to is just
        /// `refined.end_frame - refined.start_frame`, so it is not stored.
        window_gap_frames: usize,
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
        /// The total splice gain — whole-program × hole-local (`domain::stack_fill_gain`), decided
        /// at characterize time. `1.0` when the run refused the program scalar and the local half
        /// was off or declined; **below 1.0** when either half attenuates.
        normalize_gain: f32,
        /// The hole-local half of [`Self::Bracket::normalize_gain`], kept whole rather than left to
        /// be recovered as `normalize_gain / program_gain`: the reason is not a number and cannot
        /// be divided out, and `1.0` on its own cannot distinguish a collar that declined from one
        /// that ran and found the hole already matched.
        local_gain: crate::domain::LocalGainDecision,
        /// The unified fit objective's terms at `alignment.start_frame` — decided at characterize time
        /// alongside the placement itself, because only the search knows them. `None` when the gate
        /// reached its placement without the unified search (structure-only gate mode).
        fit_terms: Option<UnifiedFitBreakdown>,
        /// S1: independent shoulder-seam read of the placement vs. the applied placement, `applied_ms −
        /// shoulder_lag_ms` for whichever shoulder is more confident. Record-only — no gate reads this.
        registration_residual: Option<Box<crate::domain::patch_result::RegistrationResidual>>,
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
        /// Copied from the one `DualFitResult` (single-source, A7). `true` = a real length edit (interior-
        /// trim bridge extract); `false` = a rigid single-lag map (trim-0 extract off the pre lock,
        /// docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md §3.1). No longer gates admission — report-only.
        step_real: bool,
        /// Per-shoulder residual (A6 gate).
        residual: Option<SeamResidualVerdict>,
        confidence: FillConfidence,
        /// S1: independent shoulder-seam read vs. the applied placement, derived from `pre_lag`/`post_lag`/
        /// `pre_seam_r`/`post_seam_r` above — no new sweep. Record-only — no gate reads this.
        registration_residual: Option<Box<crate::domain::patch_result::RegistrationResidual>>,
        /// Whole-program master match for this run (`domain::program_gain`), or `1.0` when refused.
        /// Carried on the strategy — rather than read from the request at the splice — so the
        /// executor and the preview-only `outcome_from_spec` cannot disagree about what was applied.
        ///
        /// Since 2026-08-31 this is the **product**, as on [`Self::Bracket`]: the paired collar is
        /// measured on A-versus-B blocks flanking the hole and never touches the fill, so unlike the
        /// whole-fill-RMS estimator it replaced there is nothing about a dual-fit interior it cannot
        /// read. Withholding a half would leave rescued holes at a different master from bracket
        /// holes, which is why neither is simply `1.0`.
        normalize_gain: f32,
        /// The hole-local half of [`Self::SilenceSplice::normalize_gain`]. See the `Bracket` field.
        local_gain: crate::domain::LocalGainDecision,
        /// The post seam's window width (ms) when a 250 ms post lock below the admit line was
        /// re-locked at a wider rung (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2/§3.3).
        /// `None` when the post lock stayed at the 250 ms `fill_seam_search_secs` window — the
        /// common case, and the only one before this field existed.
        dual_fit_post_window_ms: Option<u32>,
        /// `--seam-extend-fill` for this arm: `cf` frames of B either side of the bridge, as
        /// `(head, tail)` interleaved PCM, so the seam blends use their own B content instead of the
        /// post seam replaying the bridge tail (`policies::apply_seam_crossfade` — see its doc for
        /// the `2·cf` duplication). `None` = flag off, or the margin does not exist, and the splice
        /// is byte-for-byte what it was.
        ///
        /// Built **here**, not at the splice as on the `Bracket` arm, because the two are in
        /// different coordinate spaces: a bracket fill is one contiguous B span, so its margins are
        /// the frames either side of a single `fill_start_frame` in **full-B**. A dual-fit bridge has
        /// no single start — each shoulder is placed at its own lag — so `head` comes from the *pre*
        /// shoulder and `tail` from the *post* shoulder, cut in the **B-extract** frames that
        /// `pre_lag`/`post_lag` are anchored in. Carrying the PCM keeps that conversion at the one
        /// site that holds both.
        seam_margin: Option<(Vec<f32>, Vec<f32>)>,
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
    /// Index into `GapReport.gaps` (stable within one scan recipe). Copied from
    /// [`FillRegion::gap_index`](crate::domain::gap_fill::FillRegion::gap_index) at characterize.
    /// Fixture specs that are not tied to a report leave this `0`.
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
            GapRepairVerdict::Patch(GapRepairStrategy::Bracket { .. }) => {
                GapRepairCell::BracketPatch
            }
            GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice { .. }) => {
                GapRepairCell::SilenceSplice
            }
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
        // Hole 2: a degenerate splice with a zero-length donor bridge. Not sub-classified — the
        // lookalike tags ride on the reason object, they do not move the cell.
        GapPatchSkipReason::ShouldersAbut(_) => GapRepairCell::SilenceSplice,
        GapPatchSkipReason::CorrelationBelowThreshold { .. } => GapRepairCell::Decorrelated,
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => GapRepairCell::ResidualVeto,
        GapPatchSkipReason::BExtractFailed
        | GapPatchSkipReason::AlignedSegmentOutOfRange
        | GapPatchSkipReason::ZeroLengthGap => GapRepairCell::Unfillable,
    }
}

/// Which cells a given skip `reason` may legitimately carry. `cell_for_skip_reason` gives the *default*;
/// characterize may sub-classify a `CorrelationBelowThreshold` to `SilenceSplice` (dual-fit declined only
/// because the flag was off) or `ProgramQuiet` (donor-dead) using richer axes. This bounds that freedom so a
/// characterize bug can't stamp a nonsensical `(cell, reason)` pair (review finding #3, e.g.
/// `Skip { cell: BracketPatch, reason: ZeroLengthGap }`). Patch-only cells (`BracketPatch`) are never
/// admissible on a skip. Wildcard-free — a new reason variant must declare its admissible set.
pub fn reason_admits_cell(reason: &GapPatchSkipReason, cell: GapRepairCell) -> bool {
    use GapRepairCell as C;
    match reason {
        GapPatchSkipReason::BoundaryAlignmentFailed => cell == C::NoPlacement,
        GapPatchSkipReason::ProgramQuiet => cell == C::ProgramQuiet,
        // Exactly one admissible cell: `program_quiet: true` on this reason is still a SilenceSplice,
        // because the decision that produced it was the bridge, not the donor level (plan §4).
        GapPatchSkipReason::ShouldersAbut(_) => cell == C::SilenceSplice,
        // The one sub-classified reason: bare = Decorrelated, or characterize's donor/seam axes narrow it.
        GapPatchSkipReason::CorrelationBelowThreshold { .. } => {
            matches!(cell, C::Decorrelated | C::SilenceSplice | C::ProgramQuiet)
        }
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => cell == C::ResidualVeto,
        GapPatchSkipReason::BExtractFailed
        | GapPatchSkipReason::AlignedSegmentOutOfRange
        | GapPatchSkipReason::ZeroLengthGap => cell == C::Unfillable,
    }
}

/// **Projection classifier (6b.2)** — reconstruct a Skip's cell from its stored D/R tags + reason, mirroring
/// the harness predicates (`dualfit_target` / `program_quiet`). Two uses:
///   * **fingerprint / golden projection** — derive the cell from tags alone (the "golden projection helper");
///   * **characterize consistency (A7)** — the cell characterize *knows* from its decisions must equal this
///     tags-derived cell, else the tags don't capture the decision (a two-oracle drift bug). Wire as a
///     `debug_assert!` when characterize builds the spec (6b.3).
///
/// Only `CorrelationBelowThreshold` needs the axes (it is the sub-classified reason); the rest map 1:1 like
/// [`cell_for_skip_reason`].
pub fn skip_cell_from_tags(reason: &GapPatchSkipReason, tags: &GapRepairTags) -> GapRepairCell {
    match reason {
        GapPatchSkipReason::BoundaryAlignmentFailed => GapRepairCell::NoPlacement,
        GapPatchSkipReason::ProgramQuiet => GapRepairCell::ProgramQuiet,
        GapPatchSkipReason::ShouldersAbut(_) => GapRepairCell::SilenceSplice,
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => GapRepairCell::ResidualVeto,
        GapPatchSkipReason::BExtractFailed
        | GapPatchSkipReason::AlignedSegmentOutOfRange
        | GapPatchSkipReason::ZeroLengthGap => GapRepairCell::Unfillable,
        GapPatchSkipReason::CorrelationBelowThreshold { .. } => {
            classify_bracket_exhausted_skip(tags)
        }
    }
}

/// Sub-classify a bracket-exhausted (`CorrelationBelowThreshold`) skip from its donor/seam axes — the
/// §2.5.2 axis table, matching the harness ordering: **program-quiet at nominal wins**, then silence-splice
/// (seams recover + real step + aligned donor bridges), then aligned-donor-dead (reported Program-quiet),
/// else decorrelated.
fn classify_bracket_exhausted_skip(tags: &GapRepairTags) -> GapRepairCell {
    // Program-quiet (nominal) wins over aligned occupancy (§2.2a / D11).
    let nominal_quiet = tags
        .donor_nominal
        .is_some_and(|d| d.silence_fraction >= crate::domain::donor::PROGRAM_QUIET_SILENCE_FRAC);
    if nominal_quiet {
        return GapRepairCell::ProgramQuiet;
    }
    let donor_bridges = tags.donor_aligned.is_some_and(|d| d.continuous);
    if let Some(sl) = &tags.seam_local {
        let step_real = sl.post_seam_r - sl.post_seam_global_r
            >= crate::domain::dual_fit::DUALFIT_STEP_REAL_MARGIN;
        if sl.gate_pass && step_real && donor_bridges {
            return GapRepairCell::SilenceSplice;
        }
        // Seams recover but the aligned donor is broken (internal silent run) — the §4.1a class-4
        // donor-dead decline, reported as Program-quiet.
        if !donor_bridges {
            return GapRepairCell::ProgramQuiet;
        }
    }
    // No seam recovery / step-not-real with an occupied donor, or seams unmeasured — decorrelated (§4.6).
    GapRepairCell::Decorrelated
}

impl GapRepairVerdict {
    /// Does this verdict mean the executor runs? The characterize→execute dispatch predicate —
    /// `Patch` specs go to `execute_region_spec`, `Skip` specs have their outcome derived from the
    /// spec (§2.5.5).
    pub fn is_patch(&self) -> bool {
        matches!(self, GapRepairVerdict::Patch(_))
    }

    /// A skip whose cell is the reason's default (`cell_for_skip_reason`). Use when characterize has no
    /// richer classification than the reason implies.
    pub fn skip(reason: GapPatchSkipReason) -> Self {
        GapRepairVerdict::Skip {
            cell: cell_for_skip_reason(&reason),
            reason,
        }
    }

    /// A skip with an explicit (sub-classified) cell — e.g. a `CorrelationBelowThreshold` narrowed to
    /// `SilenceSplice`/`ProgramQuiet`. Debug-asserts the pair is admissible ([`reason_admits_cell`]) so a
    /// characterize bug surfaces in dev rather than silently mislabelling a gap (finding #3).
    pub fn skip_with_cell(cell: GapRepairCell, reason: GapPatchSkipReason) -> Self {
        debug_assert!(
            reason_admits_cell(&reason, cell),
            "inadmissible (cell, reason) pair: {cell:?} for {reason:?}"
        );
        GapRepairVerdict::Skip { cell, reason }
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
        // Hand-added: `cases` is a fixed array, so despite this test's name it cannot detect an
        // unlisted variant — the wildcard-free match in `cell_for_skip_reason` is the real guarantee.
        let abut = GapPatchSkipReason::ShouldersAbut(crate::domain::ShouldersAbutLocks {
            pre_lag_ms: -38.0,
            post_lag_ms: -1160.3,
            bridge_frames: -48,
            gap_frames: 53_702,
            pre_z: Some(14.1),
            post_z: Some(8.2),
            pre_envelope_rescued: false,
            post_envelope_rescued: true,
            clamped: false,
            edge_pinned: false,
            program_quiet: true,
        });
        let cases = [
            (
                GapPatchSkipReason::BoundaryAlignmentFailed,
                GapRepairCell::NoPlacement,
            ),
            (
                GapPatchSkipReason::ProgramQuiet,
                GapRepairCell::ProgramQuiet,
            ),
            (corr, GapRepairCell::Decorrelated),
            (residual, GapRepairCell::ResidualVeto),
            (
                GapPatchSkipReason::BExtractFailed,
                GapRepairCell::Unfillable,
            ),
            (
                GapPatchSkipReason::AlignedSegmentOutOfRange,
                GapRepairCell::Unfillable,
            ),
            (GapPatchSkipReason::ZeroLengthGap, GapRepairCell::Unfillable),
            (abut.clone(), GapRepairCell::SilenceSplice),
        ];
        for (reason, want) in cases {
            assert_eq!(cell_for_skip_reason(&reason), want, "reason {reason:?}");
        }

        // The abut skip admits exactly one cell — the lookalike tags never move it, `program_quiet`
        // included (plan §4).
        assert!(reason_admits_cell(&abut, GapRepairCell::SilenceSplice));
        for other in [
            GapRepairCell::ProgramQuiet,
            GapRepairCell::Decorrelated,
            GapRepairCell::Unfillable,
            GapRepairCell::NoPlacement,
            GapRepairCell::ResidualVeto,
        ] {
            assert!(!reason_admits_cell(&abut, other), "abut admits {other:?}");
        }
        assert_eq!(
            skip_cell_from_tags(&abut, &GapRepairTags::default()),
            GapRepairCell::SilenceSplice,
            "the tags path agrees with the reason path"
        );
    }

    fn spec_with(verdict: GapRepairVerdict) -> GapRepairSpec {
        GapRepairSpec {
            gap_index: 0,
            a_start_secs: 0.0,
            a_end_secs: 1.0,
            gap_offset_secs: 0.0,
            refined: RefinedGapFrames {
                start_frame: 0,
                end_frame: 48_000,
            },
            b_extract: BExtractWindow {
                start_frame: 0,
                end_frame: 96_000,
                b_mapped_start_frame: 24_000,
            },
            crossfade_secs: 0.01,
            verdict,
            tags_ctx: GapRepairTags {
                registration: RegistrationTags::default(),
                seam_local: None,
                donor_nominal: None,
                donor_aligned: None,
                gate: GateTags::default(),
                levels: Some(LevelTags {
                    a_gap_floor_db: -70.0,
                    a_noise_floor_db: -60.0,
                }),
            },
        }
    }

    fn bracket() -> GapRepairStrategy {
        GapRepairStrategy::Bracket {
            alignment: FillAlignment {
                start_frame: 0,
                fill_frames: 48_000,
                pre_correlation: 0.99,
                post_correlation: 0.99,
            },
            window_gap_frames: 48_000,
            structure_start_frame: 0,
            structure_trusted: true,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted: false,
            seam_pre: 0.99,
            seam_post: 0.99,
            used_splice: false,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            normalize_gain: 1.0,
            local_gain: crate::domain::LocalGainDecision::OFF,
            fit_terms: None,
            registration_residual: None,
        }
    }

    fn silence_splice() -> GapRepairStrategy {
        GapRepairStrategy::SilenceSplice {
            fill: vec![0.0; 96_000],
            pre_seam_r: 0.98,
            post_seam_r: 0.97,
            pre_lag: 24,
            post_lag: -600,
            trim_frames: 480,
            step_real: true,
            residual: None,
            confidence: FillConfidence::High,
            registration_residual: None,
            seam_margin: None,
            normalize_gain: 1.0,
            local_gain: crate::domain::LocalGainDecision::OFF,
            dual_fit_post_window_ms: None,
        }
    }

    /// `cell()` is a total projection of the verdict — all three arms.
    #[test]
    fn cell_reads_the_stored_verdict_cell() {
        assert_eq!(
            spec_with(GapRepairVerdict::Patch(bracket())).cell(),
            GapRepairCell::BracketPatch
        );
        assert_eq!(
            spec_with(GapRepairVerdict::Patch(silence_splice())).cell(),
            GapRepairCell::SilenceSplice
        );
        let decorrelated = spec_with(GapRepairVerdict::Skip {
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

    /// The load-bearing invariant of the whole design: a Silence-splice keeps its cell whether dual-fit
    /// patched it (`Patch(SilenceSplice)`) or the flag was off and it was declined
    /// (`Skip { cell: SilenceSplice }`). Only the verdict/projection moves, never the identity.
    #[test]
    fn silence_splice_cell_is_invariant_across_dual_fit_flag() {
        let patched = spec_with(GapRepairVerdict::Patch(silence_splice()));
        let declined = spec_with(GapRepairVerdict::Skip {
            cell: GapRepairCell::SilenceSplice,
            reason: GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.98,
                post_correlation: 0.97,
                min_correlation: 0.5,
                best_attempt: None,
            },
        });
        assert_eq!(patched.cell(), GapRepairCell::SilenceSplice);
        assert_eq!(declined.cell(), GapRepairCell::SilenceSplice);
        assert_eq!(
            patched.cell(),
            declined.cell(),
            "cell must not depend on the dual_fit flag"
        );
    }

    /// `GapRepairVerdict::skip` stamps the reason's default cell; `reason_admits_cell` bounds sub-classification.
    #[test]
    fn skip_constructor_and_admissibility_guard() {
        // Default cell for a reason.
        assert!(matches!(
            GapRepairVerdict::skip(GapPatchSkipReason::ZeroLengthGap),
            GapRepairVerdict::Skip {
                cell: GapRepairCell::Unfillable,
                ..
            }
        ));

        let corr = || GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.0,
            post_correlation: 0.0,
            min_correlation: 0.0,
            best_attempt: None,
        };
        // CorrelationBelowThreshold admits its three sub-classifications...
        assert!(reason_admits_cell(&corr(), GapRepairCell::Decorrelated));
        assert!(reason_admits_cell(&corr(), GapRepairCell::SilenceSplice));
        assert!(reason_admits_cell(&corr(), GapRepairCell::ProgramQuiet));
        // ...but never a patch-only cell, and never an unrelated skip cell.
        assert!(!reason_admits_cell(&corr(), GapRepairCell::BracketPatch));
        assert!(!reason_admits_cell(&corr(), GapRepairCell::Unfillable));
        assert!(!reason_admits_cell(&corr(), GapRepairCell::NoPlacement));
        // A mechanical reason admits only Unfillable.
        assert!(reason_admits_cell(
            &GapPatchSkipReason::BExtractFailed,
            GapRepairCell::Unfillable
        ));
        assert!(!reason_admits_cell(
            &GapPatchSkipReason::BExtractFailed,
            GapRepairCell::Decorrelated
        ));
    }

    /// The finding-#3 guard fires on a nonsensical pair (debug builds).
    #[test]
    #[should_panic(expected = "inadmissible")]
    fn skip_with_cell_rejects_inadmissible_pair() {
        let _ = GapRepairVerdict::skip_with_cell(
            GapRepairCell::BracketPatch,
            GapPatchSkipReason::ZeroLengthGap,
        );
    }

    /// Consistency: the default cell from `cell_for_skip_reason` is ALWAYS admissible for its reason — the
    /// two functions can't disagree, so `GapRepairVerdict::skip` never builds an inadmissible pair.
    #[test]
    fn default_cell_is_always_admissible() {
        let reasons = [
            GapPatchSkipReason::BoundaryAlignmentFailed,
            GapPatchSkipReason::ProgramQuiet,
            GapPatchSkipReason::BExtractFailed,
            GapPatchSkipReason::AlignedSegmentOutOfRange,
            GapPatchSkipReason::ZeroLengthGap,
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                min_correlation: 0.0,
                best_attempt: None,
            },
            GapPatchSkipReason::ResidualHeadroomExceeded {
                pre_correlation: 0.0,
                post_correlation: 0.0,
                headroom_db: 0.0,
                floor_pre_db: 0.0,
                floor_post_db: 0.0,
                margin_db: 0.0,
            },
        ];
        for reason in reasons {
            assert!(
                reason_admits_cell(&reason, cell_for_skip_reason(&reason)),
                "default cell inadmissible for {reason:?}"
            );
        }
    }

    // --- 6b.2 projection classifier: `skip_cell_from_tags` across the §4.1a bracket-exhausted classes ---

    fn donor(silence_fraction: f64, continuous: bool) -> DonorInterior {
        DonorInterior {
            rms_db: -60.0,
            silence_fraction,
            longest_silence_ms: 0.0,
            continuous,
            basis: None,
        }
    }

    fn seam(gate_pass: bool, post_seam_r: f64, post_seam_global_r: f64) -> SeamLocalTags {
        SeamLocalTags {
            pre_seam_r: 0.98,
            post_seam_r,
            post_seam_global_r,
            trim_frames: 480,
            gate_pass,
            pre_lag: 24,
            post_lag: -600,
            pre_seam_prom: None,
            post_seam_prom: None,
            pre_seam_z: None,
            post_seam_z: None,
            pre_seam_phat_r: None,
            post_seam_phat_r: None,
            pre_seam_phat_prom: None,
            post_seam_phat_prom: None,
            pre_seam_phat_z: None,
            post_seam_phat_z: None,
            pre_seam_lag_ms: None,
            post_seam_lag_ms: None,
            pre_seam_phat_lag_ms: None,
            post_seam_phat_lag_ms: None,
            pre_seam_rescued: None,
            post_seam_rescued: None,
            pre_seam_edge_pinned: None,
            post_seam_edge_pinned: None,
            seams_clamped: None,
        }
    }

    /// Bracket-exhausted tags: nominal donor, aligned donor, seam-local — the axes the classifier reads.
    fn corr_tags(
        donor_nominal: Option<DonorInterior>,
        donor_aligned: Option<DonorInterior>,
        seam_local: Option<SeamLocalTags>,
    ) -> GapRepairTags {
        GapRepairTags {
            registration: RegistrationTags::default(),
            seam_local,
            donor_nominal,
            donor_aligned,
            gate: GateTags {
                brackets_total: 3,
                brackets_passing: 0,
                ..GateTags::default()
            },
            levels: Some(LevelTags {
                a_gap_floor_db: -70.0,
                a_noise_floor_db: -60.0,
            }),
        }
    }

    fn corr() -> GapPatchSkipReason {
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.0,
            post_correlation: 0.0,
            min_correlation: 0.5,
            best_attempt: None,
        }
    }

    #[test]
    fn classifier_1to1_reasons() {
        let t = corr_tags(None, None, None);
        assert_eq!(
            skip_cell_from_tags(&GapPatchSkipReason::BoundaryAlignmentFailed, &t),
            GapRepairCell::NoPlacement
        );
        assert_eq!(
            skip_cell_from_tags(&GapPatchSkipReason::ProgramQuiet, &t),
            GapRepairCell::ProgramQuiet
        );
        assert_eq!(
            skip_cell_from_tags(&GapPatchSkipReason::BExtractFailed, &t),
            GapRepairCell::Unfillable
        );
        let resid = GapPatchSkipReason::ResidualHeadroomExceeded {
            pre_correlation: 0.9,
            post_correlation: 0.9,
            headroom_db: 3.0,
            floor_pre_db: -40.0,
            floor_post_db: -40.0,
            margin_db: 1.0,
        };
        assert_eq!(skip_cell_from_tags(&resid, &t), GapRepairCell::ResidualVeto);
    }

    #[test]
    fn classifier_class3_silence_splice() {
        // seams recover (gate_pass), real step (0.98 - 0.30 ≥ 0.15), aligned donor bridges, nominal occupied.
        let t = corr_tags(
            Some(donor(0.05, false)),
            Some(donor(0.05, true)),
            Some(seam(true, 0.98, 0.30)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &t),
            GapRepairCell::SilenceSplice
        );
    }

    #[test]
    fn classifier_class4_donor_aligned_decline_is_program_quiet() {
        // Same strong seams + real step, but aligned donor BROKEN (not continuous) ⇒ Program-quiet/donor-dead.
        let t = corr_tags(
            Some(donor(0.05, false)),
            Some(donor(0.6, false)),
            Some(seam(true, 0.98, 0.30)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &t),
            GapRepairCell::ProgramQuiet
        );
    }

    #[test]
    fn classifier_class5_nominal_quiet_wins_over_good_seams() {
        // Nominal donor 100% silent ⇒ Program-quiet even though seams score well (nominal wins).
        let t = corr_tags(
            Some(donor(1.0, false)),
            Some(donor(0.05, true)),
            Some(seam(true, 0.99, 0.30)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &t),
            GapRepairCell::ProgramQuiet
        );
    }

    #[test]
    fn classifier_decorrelated_when_step_not_real() {
        // Donor bridges, seams "pass", but step is NOT real (0.98 - 0.95 < 0.15) ⇒ decorrelated (§4.6).
        let t = corr_tags(
            Some(donor(0.05, false)),
            Some(donor(0.05, true)),
            Some(seam(true, 0.98, 0.95)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &t),
            GapRepairCell::Decorrelated
        );
    }

    // --- 8b: pin the `seam_local` dependency (carried item d). `classify_bracket_exhausted_skip` can only
    // reach SilenceSplice / donor-dead-ProgramQuiet when `seam_local` is populated; with `seam_local = None`
    // it falls through to Decorrelated. That is correct ONLY under the characterize-always-detects invariant
    // (§2.5.2 — seam-local is measured for every bracket-exhausted disposition, decline included). These tests
    // lock the degradation so that if a future characterize skip path (8d/8f) forgets to populate `seam_local`,
    // the misclassification is caught here rather than silently mislabelling real silence-splice/donor-dead
    // gaps as Decorrelated in the corpus. 8a populates it on the accepted rescue path; the DECLINE path is
    // wired at 8d — until then this documents the requirement it must meet.

    #[test]
    fn classifier_none_seam_local_collapses_class3_to_decorrelated() {
        // Axes that WOULD be class-3 SilenceSplice (donor bridges, nominal occupied) but seams UNMEASURED.
        // Without `seam_local` the step-real / gate_pass test is unreachable ⇒ Decorrelated, NOT SilenceSplice.
        let would_be_class3 = corr_tags(
            Some(donor(0.05, false)),
            Some(donor(0.05, true)),
            Some(seam(true, 0.98, 0.30)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &would_be_class3),
            GapRepairCell::SilenceSplice
        );
        let seams_dropped = corr_tags(Some(donor(0.05, false)), Some(donor(0.05, true)), None);
        assert_eq!(
            skip_cell_from_tags(&corr(), &seams_dropped),
            GapRepairCell::Decorrelated,
            "unmeasured seam_local must degrade a class-3 signature to Decorrelated — characterize MUST detect"
        );
    }

    #[test]
    fn classifier_none_seam_local_collapses_class4_to_decorrelated() {
        // Axes that WOULD be class-4 donor-dead ProgramQuiet (aligned donor broken, nominal occupied) but
        // seams UNMEASURED. The `if !donor_bridges ⇒ ProgramQuiet` arm lives INSIDE `if let Some(seam_local)`,
        // so dropping seams also loses the donor-dead classification ⇒ Decorrelated.
        let would_be_class4 = corr_tags(
            Some(donor(0.05, false)),
            Some(donor(0.6, false)),
            Some(seam(true, 0.98, 0.30)),
        );
        assert_eq!(
            skip_cell_from_tags(&corr(), &would_be_class4),
            GapRepairCell::ProgramQuiet
        );
        let seams_dropped = corr_tags(Some(donor(0.05, false)), Some(donor(0.6, false)), None);
        assert_eq!(
            skip_cell_from_tags(&corr(), &seams_dropped),
            GapRepairCell::Decorrelated,
            "unmeasured seam_local must degrade a class-4 donor-dead signature to Decorrelated — characterize MUST detect"
        );
    }

    #[test]
    fn classifier_none_seam_local_still_honors_nominal_quiet() {
        // The nominal-quiet term is read BEFORE `seam_local` (it wins outright), so a program-quiet gap is
        // still correctly ProgramQuiet even with seams unmeasured — the dependency is specific to the
        // seam-recovery / donor-dead branches, not the whole classifier.
        let nominal_quiet = corr_tags(Some(donor(1.0, false)), Some(donor(0.05, true)), None);
        assert_eq!(
            skip_cell_from_tags(&corr(), &nominal_quiet),
            GapRepairCell::ProgramQuiet
        );
    }
}
