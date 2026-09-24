use serde::Serialize;

use crate::domain::fill_level::FillLevelCheck;
use crate::domain::fill_mode::FillMode;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::gap_tags::{derive_gap_tags_from_status, FillTierThresholds, GapTags};
use crate::domain::policies::SeamResidualVerdict;

/// Absent residual summary scalars on [`GapPatchStatus::Patched`] (tests / lossy builders).
#[inline]
pub const fn gap_patch_residual_unmeasured() -> (Option<f64>, Option<f64>, Option<f64>) {
    (None, None, None)
}

/// Scalar residual summary fields for patched gaps (P1 JSON).
pub fn residual_summary_scalar_fields(
    residual: Option<&SeamResidualVerdict>,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Some(v) = residual else {
        return (None, None, None);
    };
    let residual_db = v.worst_chosen_db();
    let floor_db = v.worst_floor_db();
    let headroom_db = v.worst_headroom_db();
    (
        residual_db.is_finite().then_some(residual_db),
        floor_db.is_finite().then_some(floor_db),
        headroom_db.is_finite().then_some(headroom_db),
    )
}

/// Why a detected gap was not included in the fill plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapFillSkipReason {
    NotFillable,
    TrackLayoutMismatch,
    TrackCompatibilityUnavailable,
    OutsideReferenceCoverage,
    /// Gap-equivalence gate: A's silence is already equivalent to B's (mutual/ambient silence — nothing to
    /// repair), so patching would be a no-op. Only produced when `skip_equivalent_gaps` is enabled.
    AlreadyMatchesReference,
    /// User gap selection (`--only-gaps` / `--skip-gaps`): gap would otherwise be planned but was not
    /// selected for this run.
    GapNotSelected,
}

/// Which fit-joint placement produced a seam Pearson score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamScoreSource {
    Baseline,
    Anchor,
    Grid,
    Extension,
    DualFit,
}

/// One waveform seam score from a scored placement (baseline, anchor bracket, grid cell, or extension).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SeamScoreAttempt {
    pub pre_correlation: f64,
    pub post_correlation: f64,
    pub source: SeamScoreSource,
}

impl SeamScoreAttempt {
    pub fn min_pearson(self) -> f64 {
        self.pre_correlation.min(self.post_correlation)
    }
}

pub fn format_seam_score_source(source: SeamScoreSource) -> &'static str {
    match source {
        SeamScoreSource::Baseline => "baseline",
        SeamScoreSource::Anchor => "anchor",
        SeamScoreSource::Grid => "grid",
        SeamScoreSource::Extension => "extension",
        SeamScoreSource::DualFit => "dual_fit",
    }
}

/// Pick whichever attempt has the higher `min_pearson`, keeping `a` on ties.
pub fn better_seam_score_attempt(
    a: Option<SeamScoreAttempt>,
    b: Option<SeamScoreAttempt>,
) -> Option<SeamScoreAttempt> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.min_pearson() > a.min_pearson() {
            b
        } else {
            a
        }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn format_correlation_below_threshold(
    pre_correlation: f64,
    post_correlation: f64,
    min_correlation: f32,
    best_attempt: Option<SeamScoreAttempt>,
) -> String {
    let base = format!(
        "boundary correlation below threshold (pre={pre_correlation:.2} post={post_correlation:.2} min={min_correlation:.2})"
    );
    let Some(best) = best_attempt else {
        return base;
    };
    let reported_min = pre_correlation.min(post_correlation);
    if best.min_pearson() <= reported_min + 1e-9 {
        return base;
    }
    format!(
        "{base}; best pre={:.2} post={:.2} @ {}",
        best.pre_correlation,
        best.post_correlation,
        format_seam_score_source(best.source),
    )
}

/// Why a planned gap was not patched during splice.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPatchSkipReason {
    BExtractFailed,
    BoundaryAlignmentFailed,
    CorrelationBelowThreshold {
        pre_correlation: f64,
        post_correlation: f64,
        min_correlation: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        best_attempt: Option<SeamScoreAttempt>,
    },
    AlignedSegmentOutOfRange,
    ZeroLengthGap,
    ResidualHeadroomExceeded {
        pre_correlation: f64,
        post_correlation: f64,
        headroom_db: f64,
        floor_pre_db: f64,
        floor_post_db: f64,
        margin_db: f64,
    },
    /// B is silent at the nominal program-time span (D11/G5) — quiet in both masters, nothing to fill.
    ProgramQuiet,
    /// Both A edges locked onto the same (or overlapping) B instant: the donor bridge between the two
    /// seam locks is zero-length or inverted, so B holds nothing for this gap. Measured **before** the
    /// fill search (the locks are the cheap sensor), and only on locks that clear the fill gate —
    /// unqualified locks decide nothing, because two dead shoulders produce noise lags that cross about
    /// half the time.
    ///
    /// Cell is SilenceSplice (gap-vocabulary Hole 2: degenerate splice, zero-length donor bridge). The
    /// lookalike flags are stamped, not split into separate reasons — the successor collapse Change
    /// reads them before it drains anything.
    ShouldersAbut(ShouldersAbutLocks),
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The lock geometry behind a [`GapPatchSkipReason::ShouldersAbut`] decision.
///
/// A named struct rather than inline variant fields because the identical payload is republished at
/// run level on [`ShouldersAbutSkip`] (plan §4's capture, which the successor collapse Change reads).
/// One definition, so the per-gap reason and the run-level row cannot drift apart. The wire shape is
/// unchanged by the extraction: a newtype variant still serializes as `{"shoulders_abut": {…}}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ShouldersAbutLocks {
    pub pre_lag_ms: f64,
    pub post_lag_ms: f64,
    pub bridge_frames: i64,
    pub gap_frames: usize,
    /// Whole-curve z-score of each lock's peak — uniqueness of *this* placement. Recorded, never
    /// gated on (settled § B).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_z: Option<f64>,
    /// The coarse envelope relocated this shoulder before it locked. Boolean until the three-state
    /// `EnvelopeRescue` reaches the domain (plan step 6) — `false` here means "not rescued", which
    /// covers both "never consulted" and "consulted and declined".
    #[serde(default, skip_serializing_if = "is_false")]
    pub pre_envelope_rescued: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub post_envelope_rescued: bool,
    /// The abut exists only because both seams clamped to frame 0 — near B's start, not a join.
    #[serde(default, skip_serializing_if = "is_false")]
    pub clamped: bool,
    /// Either lock's peak sat on the boundary of the lags actually searched (search exhausted).
    /// Note this does **not** mean "the true peak is out of reach" — a flat curve peaks mid-sweep.
    #[serde(default, skip_serializing_if = "is_false")]
    pub edge_pinned: bool,
    /// B is quiet at the nominal mapping — the shared-pause lookalike.
    #[serde(default, skip_serializing_if = "is_false")]
    pub program_quiet: bool,
}

/// One `shoulders_abut` refusal, carried from characterize through the splice loop and republished
/// on [`PatchSummary::shoulders_abut`].
///
/// The frames are the gap's refined throat on **A's PCM timeline**, sourced from the repair spec —
/// not from [`GapPatchOutcome`], which carries seconds and no gap identity. `splice_into_a` is
/// length-preserving, so they still name the hole after every neighbouring fill is in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ShouldersAbutSkip {
    pub gap_index: usize,
    pub a_start_frame: usize,
    pub a_end_frame: usize,
    #[serde(flatten)]
    pub locks: ShouldersAbutLocks,
}

/// Why an approved patch never reached A's PCM.
///
/// Distinct from [`GapPatchSkipReason`] in kind, not degree: a skip is the **seam gate declining**,
/// whereas this is the gate having *approved* and the splice then failing to apply the fill. Every
/// arm indicates a geometry defect upstream of the splice — the fill length or the destination
/// frames disagree with the gap the plan described — so any occurrence is a bug report, not a
/// routine outcome.
///
/// Each arm carries the numbers that identify which invariant broke, because the first real
/// occurrence is expected on media that cannot be re-run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPatchNotAppliedReason {
    /// Destination frames fall outside A's buffer.
    DestinationOutOfRange {
        gap_start_frame: usize,
        gap_end_frame: usize,
        a_frames: usize,
    },
    /// Destination is empty or inverted (`start >= end`). Silent before this variant existed.
    DestinationEmptyOrInverted {
        gap_start_frame: usize,
        gap_end_frame: usize,
    },
    /// The fill is shorter than the gap it must cover.
    FillShorterThanGap {
        fill_frames: usize,
        gap_frames: usize,
    },
}

/// Human-readable not-applied reason for stdout gap tables and verbose stderr (`-v`).
pub fn format_gap_patch_not_applied_reason(reason: &GapPatchNotAppliedReason) -> String {
    match reason {
        GapPatchNotAppliedReason::DestinationOutOfRange {
            gap_start_frame,
            gap_end_frame,
            a_frames,
        } => format!(
            "splice destination [{gap_start_frame}, {gap_end_frame}) outside A ({a_frames} frames)"
        ),
        GapPatchNotAppliedReason::DestinationEmptyOrInverted {
            gap_start_frame,
            gap_end_frame,
        } => format!("splice destination empty or inverted [{gap_start_frame}, {gap_end_frame})"),
        GapPatchNotAppliedReason::FillShorterThanGap {
            fill_frames,
            gap_frames,
        } => format!("fill {fill_frames} frames shorter than gap {gap_frames} frames"),
    }
}

/// Human-readable skip reason for stdout gap tables and verbose stderr (`-v`).
pub fn format_gap_patch_skip_reason(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::BExtractFailed => "B audio extraction failed".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "boundary alignment failed".into(),
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
            best_attempt,
        } => format_correlation_below_threshold(
            *pre_correlation,
            *post_correlation,
            *min_correlation,
            *best_attempt,
        ),
        GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned B segment out of range".into(),
        GapPatchSkipReason::ZeroLengthGap => "zero-length gap".into(),
        GapPatchSkipReason::ProgramQuiet => {
            "program-quiet in both masters (B silent at nominal span; nothing to fill)".into()
        }
        // The four geometry numbers only — the lookalike tags live on the serialized object, not in the
        // human line (plan §4).
        GapPatchSkipReason::ShouldersAbut(locks) => format!(
            "shoulders abut (bridge={} frames, gap={}, pre={:.1} ms, post={:.1} ms)",
            locks.bridge_frames, locks.gap_frames, locks.pre_lag_ms, locks.post_lag_ms
        ),
        GapPatchSkipReason::ResidualHeadroomExceeded {
            pre_correlation,
            post_correlation,
            headroom_db,
            floor_pre_db,
            floor_post_db,
            margin_db,
        } => format!(
            "residual headroom exceeded (pre={pre_correlation:.2} post={post_correlation:.2} \
             headroom={headroom_db:.1} dB floor_pre={floor_pre_db:.1} floor_post={floor_post_db:.1} \
             margin={margin_db:.1} dB)"
        ),
    }
}

/// Short skip reason for default-mode `tracing::warn` mid-run notifications.
pub fn format_gap_patch_skip_warn_reason(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::BExtractFailed => "B audio slice out of range".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "structure alignment failed".into(),
        GapPatchSkipReason::ProgramQuiet => "program-quiet (nothing to fill)".into(),
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
            best_attempt,
        } => format_correlation_below_threshold(
            *pre_correlation,
            *post_correlation,
            *min_correlation,
            *best_attempt,
        ),
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => {
            "residual headroom exceeded (anti-echo veto)".into()
        }
        other => format_gap_patch_skip_reason(other),
    }
}

/// Verbose stderr line (`-v`) aligned with per-gap fill plan indentation.
pub fn format_gap_fill_skip_verbose_line(reason: &GapPatchSkipReason) -> String {
    format!(
        "           skipped: {}",
        format_gap_patch_skip_reason(reason)
    )
}

/// Human-readable marginal patch detail for stdout gap tables and verbose stderr (`-v`).
pub fn format_gap_fill_marginal_detail(pre: f64, post: f64, min: f32, anchor_seam: bool) -> String {
    let kind = if anchor_seam {
        "marginal anchor seam"
    } else {
        "marginal waveform seam"
    };
    format!("{kind} (pre={pre:.2} post={post:.2} min={min:.2})")
}

/// Short marginal reason for default-mode `tracing::warn` mid-run notifications.
pub fn format_gap_fill_marginal_warn_reason(
    pre: f64,
    post: f64,
    min: f32,
    anchor_seam: bool,
) -> String {
    format_gap_fill_marginal_detail(pre, post, min, anchor_seam)
}

/// Verbose stderr line (`-v`) aligned with per-gap fill plan indentation.
pub fn format_gap_fill_marginal_verbose_line(
    pre: f64,
    post: f64,
    min: f32,
    anchor_seam: bool,
) -> String {
    format!(
        "           patched: {}",
        format_gap_fill_marginal_detail(pre, post, min, anchor_seam)
    )
}

/// Per-gap outcome after a patch pass (scan gaps in report order).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapPatchOutcome {
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub status: GapPatchStatus,
    /// Vocabulary tags derived at patch time (or from plan status for not-planned gaps).
    pub tags: GapTags,
    /// Residual/floor verdict (P1 report-only). Present only when residual measurement is enabled
    /// (`measure_residual` or debug logging) and the gap reached the fit waveform tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual: Option<SeamResidualVerdict>,
    /// Fill-level check (record-only): the fill's loudest bin against the A shoulders it sits
    /// between. Present only for gaps that were actually spliced, and only when
    /// `measure_fill_level` is on. Never affects the verdict — see
    /// [`crate::domain::fill_level`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_level: Option<FillLevelCheck>,
}

impl GapPatchOutcome {
    pub fn new(a_start_secs: f64, a_end_secs: f64, status: GapPatchStatus, tags: GapTags) -> Self {
        Self {
            a_start_secs,
            a_end_secs,
            status,
            tags,
            residual: None,
            fill_level: None,
        }
    }

    /// Attach a residual/floor verdict (P1 report-only); no-op when `None`.
    pub fn with_residual(mut self, residual: Option<SeamResidualVerdict>) -> Self {
        self.residual = residual;
        self
    }

    /// Attach the fill-level check (record-only); no-op when `None`.
    pub fn with_fill_level(mut self, fill_level: Option<FillLevelCheck>) -> Self {
        self.fill_level = fill_level;
        self
    }

    /// Build tags from [`GapPatchStatus`] only (lossy for patched/skipped fit gaps).
    pub fn with_tags_from_status(
        a_start_secs: f64,
        a_end_secs: f64,
        status: GapPatchStatus,
        fill_mode: FillMode,
        thresholds: FillTierThresholds,
    ) -> Self {
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        Self::new(a_start_secs, a_end_secs, status, tags)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPatchStatus {
    Patched {
        /// Seam score used for the patch decision (structure or waveform Pearson).
        pre_correlation: f64,
        post_correlation: f64,
        /// Signed B slide of the chosen fill start off mapped nominal. **Not** a lag: it is the
        /// placement the unified fit search won, not a measured A→B offset. The unified search
        /// (structure + waveform in one objective) has no separable "waveform slide" to report
        /// alongside it — a `waveform_adjustment_secs` field existed until 2026-08-12 and was
        /// identically `0.0` by construction.
        align_adjustment_secs: f64,
        /// `true` when placement was accepted from structure match without a waveform gate.
        structure_trusted: bool,
        /// Fit-mode waveform tier (`high` confident, `marginal` warn-patch band).
        confidence: FillConfidence,
        /// Frames the winning A gap start was shifted from the pre-search refined edge.
        #[serde(default)]
        gap_start_adjust_frames: i64,
        /// Frames the winning A gap end was extended from the pre-search refined edge.
        #[serde(default)]
        gap_end_adjust_frames: i64,
        /// Worst-side chosen-placement residual (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        residual_db: Option<f64>,
        /// Worst-side nominal floor (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        floor_db: Option<f64>,
        /// Worst-side headroom at chosen placement (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headroom_db: Option<f64>,
        /// Run metadata: placement won at an editorial anchor seam, not the scan throat alone
        /// (anchor-seam search rescued this gap). Mirrors `structure_trusted` as a path fact —
        /// not a `GapTags` characteristic.
        ///
        /// **Always serialized, including when `false`** (since 2026-08-12; omitted-when-false
        /// before that). `false` means baseline-throat placement — it does not mean the field is
        /// unavailable, which is how the old absence read.
        #[serde(default)]
        anchor_seam_used: bool,
        /// Frames the winning anchor bracket moved from the scan-refined baseline; 0 unless
        /// `anchor_seam_used` (the displacement of the editorial seam from the silence throat).
        #[serde(default, skip_serializing_if = "is_zero_move_frames")]
        anchor_bracket_move_frames: usize,
        /// Tail frames of the gap left unrepaired because donor B ran out before the fill reached gap
        /// length; 0 on a fill that reached it. The patch destination is shortened by this much, so A
        /// keeps its own material there rather than being overwritten with digital silence. A path
        /// fact, like `anchor_seam_used`: the shortfall used to be discarded inside the fill primitives
        /// and so appeared nowhere on the record.
        #[serde(default, skip_serializing_if = "is_zero_move_frames")]
        zero_pad_frames: usize,
        /// Run metadata: this fill was rescued by the A3 dual-fit path (G6) after the bracket
        /// search exhausted, not by ordinary bracket-search fitting. Mirrors `anchor_seam_used`
        /// as a path fact — not a `GapTags` characteristic.
        ///
        /// **Always serialized, including when `false`.** `false` means dual-fit did not carry
        /// this gap — it ran and declined, or was never reached. It has never meant "dual-fit was
        /// disabled", though the old omitted-when-false encoding was read that way.
        #[serde(default)]
        dual_fit_used: bool,
        /// When `dual_fit_used`: `true` = a real length edit (interior-trim bridge extract), `false`
        /// = a rigid single-lag map (trim-0 extract off the pre lock,
        /// docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md §3.1). `None` on every non-dual-fit path, and
        /// on runs predating this field — not recoverable from anything else on the record.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dual_fit_step_real: Option<bool>,
        /// When `dual_fit_used`: the post seam's window width (ms) when a 250 ms post lock below
        /// the admit line was re-locked at a wider rung
        /// (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2/§3.3). `None` when the post lock stayed
        /// at the 250 ms `fill_seam_search_secs` window, and on every non-dual-fit path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dual_fit_post_window_ms: Option<u32>,
        /// **Total** loudness gain applied to this fill before the splice — whole-program ×
        /// hole-local (`domain::stack_fill_gain`), which is what [`Self::program_gain`] exists to
        /// let a reader factor apart. `1.0` means the donor level was written untouched: the run
        /// refused the program scalar *and* the local half was off or declined.
        ///
        /// **It can sit below 1.0 since 2026-08-26**, and since 2026-08-31 either half can put it
        /// there. The program half has always attenuated a louder donor by design (plan § 3); the
        /// local half was boost-only from 2026-08-12 to 2026-08-31, when the paired collar replaced
        /// `policies::compute_fill_gain` and the floor went with it — a scalar taken from paired
        /// shoulder blocks cannot crush a loud mid-gap event the way whole-fill RMS could, which is
        /// the entire reason the floor existed. Runs before 2026-08-12 could also attenuate
        /// locally, down to `-max_fill_gain_db` — a knob removed on 2026-08-31 with the estimator it bounded.
        /// Neither half bounds the product: `max_program_gain_db` bounds the program half and
        /// `MAX_LOCAL_RESIDUAL_DB` (3.0) the local one, so a run can reach their sum before the
        /// splice clamps samples.
        ///
        /// **Always serialized.** It exists to make the `fill_level` fields interpretable: those are
        /// measured on `gained_fill`, so without the gain there is no way to tell a loud fill from a
        /// boosted quiet one, or to compare `peak_delta_db` / `edge_delta_db` across a gain-policy
        /// change.
        ///
        /// Absence on a record means **the run predates the field** (before 2026-08-12), not
        /// "unity". This type is `Serialize`-only — nothing here reads a report back — so the
        /// hazard is entirely on the consumer side: `gap-files/` dumps of mixed vintage will have
        /// gaps with no `normalize_gain` at all, and a roll-up must hold those out rather than
        /// coerce them to a value.
        normalize_gain: f32,
        /// The **whole-program** half of `normalize_gain` — A↔B master match from the scan's paired
        /// offset (`domain::program_gain`), `1.0` when the run refused it or `--no-program-gain`.
        ///
        /// A run constant, repeated on every gap so a single patched row is self-describing. Why
        /// this run applied (or refused) the value it did is stamped once on
        /// [`PatchSummary::program_gain`], not here.
        ///
        /// **Always serialized** (this type is serialize-only, so there is no serde default to
        /// lean on). Absence in a dump means the run predates 2026-08-26, not unity.
        program_gain: f32,
        /// The **hole-local** half of [`Self::Patched::normalize_gain`] — the paired-collar residual
        /// against the applied program offset (`domain::local_gain`), `1.0` on any abstention or
        /// refusal. Per gap, unlike [`Self::Patched::program_gain`].
        ///
        /// Until 2026-08-31 this was recoverable as `normalize_gain / program_gain` and no field
        /// existed. It has one now because the division is lossy in the direction that matters: a
        /// quotient of exactly 1.0 covers *both* "the collar declined" and "the collar ran and found
        /// this hole already matched", and those are different facts about the gap — the first says
        /// the estimator was absent, the second that it agreed. A corpus that reads mostly the
        /// former has not had its local gain replaced, it has had it deleted, and that has to be
        /// visible without a second run. [`Self::Patched::local_gain_reason`] is which.
        ///
        /// **Always serialized.** Absence means the run predates 2026-08-31 — on such a run the
        /// quotient is still the right recovery, and it is still the local *boost* estimator, not
        /// this one.
        local_gain: f32,
        /// Why [`Self::Patched::local_gain`] is what it is, including on success.
        ///
        /// `applied` and any of the abstentions can all carry `local_gain: 1.0`; only this field
        /// separates them. `flag_off` is the default run's answer (`--normalize` is off), and
        /// `abstained_n` on a run with the flag on means the collar was too thin — a `--normalize`
        /// roll-up should report the mix before it reports the gains.
        local_gain_reason: crate::domain::LocalGainReason,
        /// The unified fit objective's terms at the placement that was applied — `structure_combined`,
        /// `waveform_min`, and each prior's subtracted amount (see
        /// [`crate::domain::gap_fill_fit::UnifiedFitBreakdown`]).
        ///
        /// This is the *why* behind `align_adjustment_secs`: that field says where the fill landed, this
        /// one says which term paid for it. Absent on non-bracket paths (dual-fit, splice), which do not
        /// run the unified objective, and on runs predating 2026-08-12.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fit_terms: Option<crate::domain::gap_fill_fit::UnifiedFitBreakdown>,
        /// S1: independent shoulder-seam reads of the placement — see [`RegistrationResidual`].
        /// Flattened, so the wire keeps `registration_residual_ms` where it always was and adds the
        /// per-seam keys beside it. `None` (no keys at all) when neither shoulder is confident.
        /// Record-only — no gate reads this field.
        #[serde(flatten)]
        registration_residual: Option<RegistrationResidual>,
    },
    Skipped {
        reason: GapPatchSkipReason,
    },
    NotPlanned {
        reason: GapFillSkipReason,
    },
    /// The seam gate approved this gap and a fill was built, but the splice never reached A's PCM.
    /// A is byte-identical to its input across the gap. Always a geometry bug upstream of the
    /// splice — see [`GapPatchNotAppliedReason`].
    NotApplied {
        reason: GapPatchNotAppliedReason,
    },
}

/// S1 registration read of one patched gap: each licensed shoulder's independent seam-local lag
/// against the placement actually written, `applied_ms − shoulder_lag_ms` (ms, signed; positive =
/// the fill sits late of that shoulder). Record-only — no gate reads it. A behaviourless record
/// serialized directly (`docs/json-output.md` § Revision procedure): field names are the wire keys,
/// and the selection lives with its producer (`region::dual_fit::registration_residual_from_shoulders`).
///
/// `registration_residual_ms` is the historical key: the licensed shoulder that agrees more closely
/// with the placement (smaller `|·|`, pre on a tie). The per-seam keys (added
/// 2026-09-22, `docs/dev/TEMP-dual-fit-registration-routing-plan.md` § 3) keep the other shoulder,
/// which the selection used to discard — across a map step the two differ by the step, and the
/// farther seam is the one a misplaced fill fails on. A per-seam key is absent when that shoulder's
/// peak is below the confidence floor.
///
/// `registration_applied_ms` is the minuend both residuals share: `align_adjustment_secs × 1000` plus Fit's
/// trim-head move, which `align_adjustment_secs` does not carry. `0.0` on dual-fit, matching that
/// path's recorded `align_adjustment_secs`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RegistrationResidual {
    pub registration_residual_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_residual_pre_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_residual_post_ms: Option<f64>,
    pub registration_applied_ms: f64,
}

/// Skip serializing `anchor_bracket_move_frames` for non-anchor patches (the common case).
fn is_zero_move_frames(frames: &usize) -> bool {
    *frames == 0
}

/// Skip serializing `not_applied_count` on healthy runs (the only expected value is 0).
fn is_zero_count(count: &usize) -> bool {
    *count == 0
}

/// Border-geometry **settings** this run patched with, echoed per run so a differential
/// across two runs is attributable to the setting that changed rather than to the directory it landed
/// in. Added for the 2026-08-17 `border_standoff_secs` sweep, where nothing in the report recorded the
/// swept value and the arms were distinguishable only by their output path — a run whose result cannot
/// be told apart from a mis-applied flag.
///
/// Settings, not per-gap effective widths: each field is read by the patch path
/// (`patch_audio::geometry`, `patch_audio::region`) but is then clamped against the material actually
/// available at that gap, so the template a given gap got can be narrower than the value here. Read it
/// as "what this run was asked for", never as "what gap *n* used".
///
/// The
/// fingerprint probe's geometry is *not* here and must not be added: it is different, and it has its
/// own block (`gap_fingerprint::CorpusProbeRecipe`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FillGeometryRecipe {
    /// How far the border templates are held off the dropout edge (`--border-standoff-secs`).
    pub border_standoff_secs: f64,
    /// How much border material the fill search may discover / requires.
    pub fill_border_search_secs: f64,
    pub min_border_discovery_secs: f64,
    /// Seam-scoring window either side of the splice.
    pub fill_seam_search_secs: f64,
    /// The **A-border template width** — `border_frames_from_secs(normalize_window_secs)` in
    /// `patch_audio::geometry`, which also floors `correlate_frames`. Named for normalization but read
    /// as a border size on this path, so a standoff sweep that omitted it would be describing the
    /// standoff of a template whose width it never recorded.
    pub normalize_window_secs: f64,
    /// `--residual-border-standoff-secs`: the standoff `registration_residual_ms` was **measured**
    /// with, when it differs from placement's. Omitted when unset (the residual then reads with
    /// `border_standoff_secs`), so default output is unchanged. Added 2026-09-19 so the fixed-instrument
    /// standoff sweep's arms are told apart by the report, not by their directory names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual_border_standoff_secs: Option<f64>,
}

/// User-visible summary of a `PatchAudio` run (no PCM payload).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatchSummary {
    pub patched_count: usize,
    pub patched_marginal_count: usize,
    pub skipped_count: usize,
    pub not_planned_count: usize,
    /// Gaps the gate approved but the splice failed to apply. Non-zero means a bug; omitted from
    /// JSON when zero, so a healthy run's output is unchanged by this field's existence.
    #[serde(default, skip_serializing_if = "is_zero_count")]
    pub not_applied_count: usize,
    pub gaps: Vec<GapPatchOutcome>,
    /// Run-level donor relationship inferred from informative-floor fraction (P4 diagnostic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub donor_relation: Option<crate::domain::gap_tags::DonorRelation>,
    /// Offset anchors built from pass-1 successes (`anchored_retry`); omitted when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_anchors_used: Option<Vec<crate::domain::patch_anchor::PatchAnchorReport>>,
    /// Border geometry this run used — see [`FillGeometryRecipe`]. `None` on summaries built without a
    /// request in hand (domain tests), and absent from pre-2026-08-17 reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_geometry: Option<FillGeometryRecipe>,
    /// What this run decided about whole-program fill gain, and the measurement it decided from —
    /// stamped **once**, not per gap. `None` on summaries built without a request in hand (domain
    /// tests), and absent from pre-2026-08-26 reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_gain: Option<crate::domain::ProgramGainReport>,
    /// Every gap refused as `shoulders_abut`, with the A frames its silence occupies — the run-level
    /// half of plan §4's capture. Omitted when empty, so a run without an abut is byte-identical to
    /// one written before this field existed. Recorded here (rather than left to be re-derived from
    /// `gaps`) because the successor collapse Change needs the A frames, which the per-gap status
    /// does not carry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shoulders_abut: Option<Vec<ShouldersAbutSkip>>,
    /// File-level injected-time finding (A4). Omitted when the pre-pass does not flag, so existing
    /// goldens stay byte-identical until a run actually warns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub injected_time: Option<crate::domain::injected_time::InjectedTimeReport>,
}

impl PatchSummary {
    /// True when at least one gap was spliced into A's PCM.
    pub fn has_patches(&self) -> bool {
        self.patched_count > 0
    }

    pub fn from_outcomes(gaps: Vec<GapPatchOutcome>) -> Self {
        let donor_relation = crate::domain::gap_tags::derive_donor_relation(&gaps);
        let mut patched_count = 0usize;
        let mut patched_marginal_count = 0usize;
        let mut skipped_count = 0usize;
        let mut not_planned_count = 0usize;
        let mut not_applied_count = 0usize;
        for gap in &gaps {
            match gap.status {
                GapPatchStatus::Patched { confidence, .. } => {
                    patched_count += 1;
                    if confidence == FillConfidence::Marginal {
                        patched_marginal_count += 1;
                    }
                }
                GapPatchStatus::Skipped { .. } => skipped_count += 1,
                GapPatchStatus::NotPlanned { .. } => not_planned_count += 1,
                GapPatchStatus::NotApplied { .. } => not_applied_count += 1,
            }
        }
        Self {
            patched_count,
            patched_marginal_count,
            skipped_count,
            not_planned_count,
            not_applied_count,
            gaps,
            donor_relation,
            patch_anchors_used: None,
            fill_geometry: None,
            program_gain: None,
            shoulders_abut: None,
            injected_time: None,
        }
    }

    /// Stamp the border geometry this run used — see [`FillGeometryRecipe`].
    pub fn with_fill_geometry(mut self, geometry: FillGeometryRecipe) -> Self {
        self.fill_geometry = Some(geometry);
        self
    }

    /// Stamp the run's program-gain decision. See [`Self::program_gain`].
    pub fn with_program_gain(mut self, report: crate::domain::ProgramGainReport) -> Self {
        self.program_gain = Some(report);
        self
    }

    /// Stamp the run's `shoulders_abut` refusals. See [`Self::shoulders_abut`]; empty stays `None`.
    pub fn with_shoulders_abut(mut self, skips: Vec<ShouldersAbutSkip>) -> Self {
        self.shoulders_abut = (!skips.is_empty()).then_some(skips);
        self
    }

    /// Stamp the injected-time pre-pass. See [`Self::injected_time`]; `None` stays omitted.
    pub fn with_injected_time(
        mut self,
        report: Option<crate::domain::injected_time::InjectedTimeReport>,
    ) -> Self {
        self.injected_time = report;
        self
    }

    pub fn with_patch_anchors(
        mut self,
        anchors: Vec<crate::domain::patch_anchor::PatchAnchorReport>,
    ) -> Self {
        if anchors.is_empty() {
            self.patch_anchors_used = None;
        } else {
            self.patch_anchors_used = Some(anchors);
        }
        self
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn gap_not_selected_serializes_as_snake_case_json() {
        let json = serde_json::to_string(&GapFillSkipReason::GapNotSelected).expect("serialize");
        assert_eq!(json, "\"gap_not_selected\"");
    }

    #[test]
    fn correlation_skip_includes_best_attempt_when_higher() {
        let reason = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.06,
            post_correlation: 0.06,
            min_correlation: 0.12,
            best_attempt: Some(SeamScoreAttempt {
                pre_correlation: 0.18,
                post_correlation: 0.31,
                source: SeamScoreSource::Anchor,
            }),
        };
        let text = format_gap_patch_skip_reason(&reason);
        assert!(text.contains("pre=0.06 post=0.06"));
        assert!(text.contains("best pre=0.18 post=0.31 @ anchor"));
    }

    #[test]
    fn gap_fill_skip_verbose_line_matches_stdout_status() {
        let reason = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.23,
            post_correlation: 0.21,
            min_correlation: 0.35,
            best_attempt: None,
        };
        assert_eq!(
            format_gap_fill_skip_verbose_line(&reason),
            "           skipped: boundary correlation below threshold (pre=0.23 post=0.21 min=0.35)"
        );
    }

    #[test]
    fn gap_fill_marginal_verbose_line_matches_stdout_status() {
        assert_eq!(
            format_gap_fill_marginal_verbose_line(0.31, 1.0, 0.35, false),
            "           patched: marginal waveform seam (pre=0.31 post=1.00 min=0.35)"
        );
        assert_eq!(
            format_gap_fill_marginal_verbose_line(0.28, 0.9, 0.35, true),
            "           patched: marginal anchor seam (pre=0.28 post=0.90 min=0.35)"
        );
    }

    #[test]
    fn gap_patch_skip_warn_reason_keeps_legacy_short_phrases() {
        assert_eq!(
            format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BoundaryAlignmentFailed),
            "structure alignment failed"
        );
        assert_eq!(
            format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BExtractFailed),
            "B audio slice out of range"
        );
    }

    /// Regression for the 2026-07 production report: when the ordinary bracket search's best
    /// attempt fails the gate, but dual-fit's own (also-failed) re-validation scored higher, the
    /// final skip report must surface dual-fit's attempt instead of silently dropping it — the
    /// ordinary anchor-bracket report previously never reflected anything dual-fit tried.
    #[test]
    fn better_seam_score_attempt_prefers_the_higher_min_pearson() {
        let anchor_attempt = SeamScoreAttempt {
            pre_correlation: 0.13,
            post_correlation: 0.09,
            source: SeamScoreSource::Anchor,
        };
        let dual_fit_attempt = SeamScoreAttempt {
            pre_correlation: 0.09,
            post_correlation: 0.007,
            source: SeamScoreSource::DualFit,
        };
        // Dual-fit's own re-validation, in this example, scored WORSE than the anchor bracket's
        // best attempt — the anchor attempt must still win and be reported.
        let best = better_seam_score_attempt(Some(anchor_attempt), Some(dual_fit_attempt));
        assert_eq!(best, Some(anchor_attempt));

        let stronger_dual_fit = SeamScoreAttempt {
            pre_correlation: 0.5,
            post_correlation: 0.4,
            source: SeamScoreSource::DualFit,
        };
        let best = better_seam_score_attempt(Some(anchor_attempt), Some(stronger_dual_fit));
        assert_eq!(best, Some(stronger_dual_fit));

        // No ordinary best_attempt at all (e.g. `StructureBelowThreshold`, which hard-codes
        // `None`) — dual-fit's attempt should still surface on its own.
        let best = better_seam_score_attempt(None, Some(stronger_dual_fit));
        assert_eq!(best, Some(stronger_dual_fit));

        // dual-fit never ran / was ineligible — nothing to merge in, ordinary result unchanged.
        let best = better_seam_score_attempt(Some(anchor_attempt), None);
        assert_eq!(best, Some(anchor_attempt));
    }

    #[test]
    fn format_seam_score_source_covers_dual_fit() {
        assert_eq!(
            format_seam_score_source(SeamScoreSource::DualFit),
            "dual_fit"
        );
    }
}
