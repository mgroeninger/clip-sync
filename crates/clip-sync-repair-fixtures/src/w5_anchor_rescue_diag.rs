//! W5 anchor-rescue discovery diagnostic (Phases 1–2).
//! See docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md.
//!
//! **Phase 1** ([`score_w5_anchor_rescue_cell`]): for one cell scores the symmetric-weak-throat
//! fixture's nominal + baseline throat Pearson plus every feasible anchor bracket on the **unified
//! haystack gate path** — the same path `AudioFitSource::score` drives in production (via
//! [`oracle_score_fit_candidate`]).
//!
//! **Phase 2** ([`evaluate_w5_cell`] / [`classify_w5_cell`]): adds the **production-faithful joint
//! winner** (full E1–E7 routing on oracle-built params, no decode) and a regime label so a grid
//! sweep can locate the E3 (anchor-rescue) pocket. Diagnostic tier — emits data, no PR gate.

use std::time::Instant;

use clip_sync::MultiChannelPcm;

use clip_sync_repair::application::gate_oracle::{
    derive_seam_gate_geometry, oracle_anchor_seam_would_run, oracle_build_fit_cache,
    oracle_evaluate_fit_joint, oracle_score_fit_candidate, OracleJointOutcome, SeamGateDerived,
    SeamGateFailure, SeamGateParams,
};
use clip_sync_repair::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorBracket, AnchorSeamMode,
    AnchorSeamParams,
};
use clip_sync_repair::domain::policies::RefinedGapFrames;
use clip_sync_repair::domain::FillConfidence;
use clip_sync_repair::infrastructure::config::RepairConfig;
use crate::energy_signature_fixtures::{
    build_w5_symmetric_weak_throat_anchor_rescue_with_b_shift, gap_report_times,
    EnergySignatureFixture,
};
use crate::energy_signature_production::{
    gap_report_from_energy_fixture, oracle_baseline_throat_pearson_opt,
    oracle_nominal_throat_pearson, patch_request_from_repair, production_geometry_params,
};
use crate::patch_geometry_preview::{preview_patch_geometry, slice_b_interleaved};

/// Pearson High floor (`min_fill_correlation`) for the A6 oracle.
const HIGH_FLOOR: f64 = 0.35;

/// One W5 discovery cell (Phase 1+). `fill_border_search_secs` must be `< peak_offset_secs` for the
/// fixture to be valid (baseline unified search must not High-short-circuit at the throat).
///
/// `b_shift_secs` is the B dropout shift (where the fill lives on B). `None` couples it to
/// `peak_offset_secs` (the original A6 fixture); `Some(x)` decouples it (§8 Q1 — see
/// [`build_w5_symmetric_weak_throat_anchor_rescue_with_b_shift`]).
#[derive(Debug, Clone, Copy)]
pub struct W5AnchorRescueCell {
    /// A peak offset — where the editorial anchors sit relative to the throat (F1-style).
    pub peak_offset_secs: f64,
    /// Repair + structure `search_radius`; must be `< peak_offset_secs`.
    pub fill_border_search_secs: f64,
    /// B dropout shift; `None` = coupled (`= peak_offset_secs`).
    pub b_shift_secs: Option<f64>,
}

impl Default for W5AnchorRescueCell {
    fn default() -> Self {
        Self {
            peak_offset_secs: 1.0,
            fill_border_search_secs: 0.78,
            b_shift_secs: None,
        }
    }
}

impl W5AnchorRescueCell {
    /// Coupled cell `(peak_offset, search)` — `b_shift` tracks `peak_offset`.
    pub fn coupled(peak_offset_secs: f64, fill_border_search_secs: f64) -> Self {
        Self {
            peak_offset_secs,
            fill_border_search_secs,
            b_shift_secs: None,
        }
    }

    /// Effective B dropout shift in seconds (resolves the coupled default).
    pub fn effective_b_shift_secs(&self) -> f64 {
        self.b_shift_secs.unwrap_or(self.peak_offset_secs)
    }

    /// `fill_border_search_secs >= peak_offset_secs` — the fixture is degenerate (baseline can High).
    pub fn is_invalid(&self) -> bool {
        self.fill_border_search_secs >= self.peak_offset_secs
    }
}

/// Paired fixture + repair for one cell (`anchor_seam_mode = Auto`).
pub fn build_w5_cell(cell: &W5AnchorRescueCell) -> (EnergySignatureFixture, RepairConfig) {
    let fixture = build_w5_symmetric_weak_throat_anchor_rescue_with_b_shift(
        48_000,
        1,
        cell.peak_offset_secs,
        cell.fill_border_search_secs,
        cell.effective_b_shift_secs(),
    );
    let repair = crate::energy_signature_production::w5_anchor_rescue_repair(
        AnchorSeamMode::Auto,
        cell.fill_border_search_secs,
    );
    (fixture, repair)
}

/// Anchor-search params; `min_prominence` comes from the repair so the diagnostic enumeration
/// matches what the production joint routing admits.
fn anchor_params(fixture: &EnergySignatureFixture, min_prominence: f32) -> AnchorSeamParams {
    AnchorSeamParams {
        context_frames: fixture.context_frames,
        max_anchors_per_side: 5,
        max_bracket_frames: (5.0 * fixture.sample_rate as f64).round() as usize,
        min_prominence,
        structure: fixture.structure_params,
    }
}

/// One anchor bracket scored on the unified gate path. `pre_pearson`/`post_pearson` carry the seam
/// Pearson the gate measured **whether or not it passed** (so a failing bracket still shows its
/// scores); `failure_stage` names which gate stage rejected it when `!passed_gate`.
#[derive(Debug, Clone)]
pub struct W5BracketGateScore {
    pub pre_frame: usize,
    pub post_frame: usize,
    pub move_frames: usize,
    /// Energy-peak prominence of the pre/post anchors (`0` for scan-fallback). Drives candidate
    /// filtering via `anchor_seam_min_prominence`.
    pub pre_prominence: f32,
    pub post_prominence: f32,
    pub passed_gate: bool,
    pub pre_pearson: Option<f64>,
    pub post_pearson: Option<f64>,
    /// `min(pre, post)` when the gate passed.
    pub min_pearson: Option<f64>,
    pub confidence: Option<FillConfidence>,
    pub ranking_score: Option<f64>,
    /// Which gate stage rejected the bracket (`None` when it passed). One of `structure_align`,
    /// `structure_floor`, `waveform_floor`, `residual`.
    pub failure_stage: Option<&'static str>,
}

/// Gate stage + measured pre/post for a rejection. `structure_align` carries no scores.
fn failure_detail(failure: &SeamGateFailure) -> (&'static str, Option<f64>, Option<f64>) {
    match failure {
        SeamGateFailure::StructureAlignmentFailed => ("structure_align", None, None),
        SeamGateFailure::StructureBelowThreshold { pre, post } => {
            ("structure_floor", Some(*pre), Some(*post))
        }
        SeamGateFailure::WaveformBelowThreshold { pre, post, .. } => {
            ("waveform_floor", Some(*pre), Some(*post))
        }
        SeamGateFailure::ResidualHeadroomExceeded { pre, post, .. } => {
            ("residual", Some(*pre), Some(*post))
        }
    }
}

/// All Phase 1 scores for one cell.
#[derive(Debug, Clone)]
pub struct W5AnchorRescueCellScores {
    pub cell: W5AnchorRescueCell,
    pub nominal_pre: f64,
    pub nominal_post: f64,
    pub baseline_pre: f64,
    pub baseline_post: f64,
    pub brackets: Vec<W5BracketGateScore>,
    pub wall_ms: u64,
}

impl W5AnchorRescueCellScores {
    /// Best `min(pre, post)` among brackets that passed the gate.
    pub fn max_bracket_min(&self) -> Option<f64> {
        self.brackets
            .iter()
            .filter(|b| b.passed_gate)
            .filter_map(|b| b.min_pearson)
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v))))
    }

    /// Count of brackets that passed the gate.
    pub fn passing_bracket_count(&self) -> usize {
        self.brackets.iter().filter(|b| b.passed_gate).count()
    }

    /// Baseline throat `min(pre, post)`.
    pub fn baseline_min(&self) -> f64 {
        self.baseline_pre.min(self.baseline_post)
    }
}

/// Which candidate won the production joint pool (Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum W5JointWinner {
    /// Whole gate skipped (E5) — nothing patched.
    Skip,
    /// Baseline throat won without engaging the anchor path.
    Baseline,
    /// An editorial anchor bracket won (`anchor_seam_used`), moving `move_frames` from baseline.
    Anchor { move_frames: usize },
}

/// Behavioral regime for a cell (Phase 2 classifier target — plan §4/§5.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum W5AnchorRescueRegime {
    /// `fill_border_search_secs >= peak_offset_secs`.
    Invalid,
    /// Baseline gate fails and no bracket passes.
    AllSkip,
    /// Baseline throat reaches High (≥ 0.35).
    BaselineHigh,
    /// Not High; a bracket reaches High and the anchor path wins the pool — the E3 pocket.
    AnchorRescuePossible,
    /// Baseline (marginal) wins the pool without the anchor path.
    BaselineMarginalWins,
}

impl W5AnchorRescueRegime {
    pub fn as_str(&self) -> &'static str {
        match self {
            W5AnchorRescueRegime::Invalid => "Invalid",
            W5AnchorRescueRegime::AllSkip => "AllSkip",
            W5AnchorRescueRegime::BaselineHigh => "BaselineHigh",
            W5AnchorRescueRegime::AnchorRescuePossible => "AnchorRescuePossible",
            W5AnchorRescueRegime::BaselineMarginalWins => "BaselineMarginalWins",
        }
    }
}

/// Full Phase 2 evaluation of one cell: Phase 1 scores + production joint winner + would-run flag.
#[derive(Debug, Clone)]
pub struct W5CellEvaluation {
    pub scores: W5AnchorRescueCellScores,
    pub joint_winner: W5JointWinner,
    pub anchor_seam_would_run: bool,
}

fn pcm_from_samples(samples: Vec<f32>, sample_rate: u32, channels: usize) -> MultiChannelPcm {
    MultiChannelPcm {
        sample_rate,
        channels: channels as u16,
        samples,
        decode_error_skips: 0,
        decoded_frame_count: None,
        compressed_bytes: None,
        source_bit_depth: None,
    }
}

/// Owns the buffers a [`SeamGateParams`] borrows so a cell can be scored repeatedly (per bracket,
/// then through the joint routing). Built once per cell from the fixture's production geometry.
struct W5CellContext {
    a_pcm: MultiChannelPcm,
    b_haystack: Vec<f32>,
    settings: clip_sync_repair::application::PatchRequestSettings,
    derived: SeamGateDerived,
    b_extract_start_secs: f64,
    refined_b_start_secs: f64,
    refined_b_end_secs: f64,
    gap_frames: usize,
    baseline: RefinedGapFrames,
    nominal: (f64, f64),
    baseline_throat: (f64, f64),
    brackets: Vec<AnchorBracket>,
}

impl W5CellContext {
    /// Build the seam-gate params for this cell (geom rebuilt from the owned buffers; baseline
    /// geometry — candidate deltas are applied inside the gate). This is the oracle counterpart of
    /// production's `seam_gate_params_from_energy_fixture` (plan §5.1b), built from the **shared**
    /// Phase 0 constructors so it cannot drift from production.
    fn params(&self) -> SeamGateParams<'_> {
        let geom = derive_seam_gate_geometry(
            &self.settings,
            &self.derived,
            &self.a_pcm,
            &self.b_haystack,
            self.b_extract_start_secs,
            self.refined_b_start_secs,
            self.refined_b_end_secs,
            self.gap_frames,
            None,
        );
        SeamGateParams {
            settings: &self.settings,
            derived: self.derived,
            geom,
        }
    }
}

/// Build the per-cell scoring context. Returns `None` for a **degenerate** cell whose baseline
/// unified haystack match fails (empty extract window / no match) — the sweep records those as a
/// skip rather than aborting (plan §5.2.8 robustness).
fn build_w5_cell_context(cell: &W5AnchorRescueCell) -> Option<W5CellContext> {
    let (fixture, repair) = build_w5_cell(cell);
    context_from_fixture(&fixture, &repair)
}

/// Scoring context for an arbitrary fixture + repair (used by both the cell grid and ad-hoc fixture
/// probes such as the noise-collar variant). `None` if the baseline unified match is degenerate.
fn context_from_fixture(
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
) -> Option<W5CellContext> {
    let baseline_throat = oracle_baseline_throat_pearson_opt(fixture, repair)?;
    let nominal = oracle_nominal_throat_pearson(fixture, repair);

    // Production-equivalent per-gap geometry (B window, refined gap) for the haystack gate path.
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), fixture);
    let (a_start, a_end, b_start, b_end, _total) = gap_report_times(fixture);
    let preview = preview_patch_geometry(
        fixture,
        &report.alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &production_geometry_params(repair),
    );
    let silence_peak_fraction = report.silence_peak_fraction;
    let request = patch_request_from_repair(report, repair);

    let ch = fixture.channels.max(1);
    let derived = clip_sync_repair::application::gate_oracle::seam_gate_derived_from_repair(
        &request,
        fixture.sample_rate,
        fixture.channels,
        silence_peak_fraction,
    );
    let a_pcm = pcm_from_samples(fixture.a_samples.clone(), fixture.sample_rate, fixture.channels);
    let b_haystack = slice_b_interleaved(
        &fixture.b_samples,
        ch,
        fixture.sample_rate,
        preview.b_extract_start_secs,
        preview.b_extract_end_secs,
    );
    let baseline = preview.refined;
    let gap_frames = baseline.end_frame.saturating_sub(baseline.start_frame);

    // Feasible brackets relative to the gate baseline (same helpers as the A6 domain oracle);
    // honor the repair's prominence floor so the enumeration matches the production joint.
    let aparams = anchor_params(fixture, repair.anchor_seam_min_prominence);
    let candidates =
        list_anchor_candidates_a(&fixture.a_samples, fixture.channels, baseline, &aparams);
    let brackets = list_feasible_anchor_brackets(&candidates, baseline, &aparams);

    Some(W5CellContext {
        a_pcm,
        b_haystack,
        settings: request.settings,
        derived,
        b_extract_start_secs: preview.b_extract_start_secs,
        refined_b_start_secs: preview.refined_b_start_secs,
        refined_b_end_secs: preview.refined_b_end_secs,
        gap_frames,
        baseline,
        nominal,
        baseline_throat,
        brackets,
    })
}

/// Scores for a degenerate cell (baseline match failed): NaN throat scores, no brackets.
fn degenerate_scores(cell: W5AnchorRescueCell, wall_ms: u64) -> W5AnchorRescueCellScores {
    W5AnchorRescueCellScores {
        cell,
        nominal_pre: f64::NAN,
        nominal_post: f64::NAN,
        baseline_pre: f64::NAN,
        baseline_post: f64::NAN,
        brackets: Vec::new(),
        wall_ms,
    }
}

fn score_brackets(ctx: &W5CellContext) -> Vec<W5BracketGateScore> {
    let params = ctx.params();
    let cache = oracle_build_fit_cache(&params);
    ctx.brackets
        .iter()
        .map(|bracket| {
            match oracle_score_fit_candidate(&params, &cache, bracket.refined, ctx.baseline, true) {
                Ok(s) => W5BracketGateScore {
                    pre_frame: bracket.pre.frame,
                    post_frame: bracket.post.frame,
                    move_frames: bracket.move_frames,
                    pre_prominence: bracket.pre.prominence,
                    post_prominence: bracket.post.prominence,
                    passed_gate: true,
                    pre_pearson: Some(s.report_pre),
                    post_pearson: Some(s.report_post),
                    min_pearson: Some(s.report_pre.min(s.report_post)),
                    confidence: Some(s.confidence),
                    ranking_score: Some(s.ranking_score),
                    failure_stage: None,
                },
                Err(failure) => {
                    let (stage, pre, post) = failure_detail(&failure);
                    W5BracketGateScore {
                        pre_frame: bracket.pre.frame,
                        post_frame: bracket.post.frame,
                        move_frames: bracket.move_frames,
                        pre_prominence: bracket.pre.prominence,
                        post_prominence: bracket.post.prominence,
                        passed_gate: false,
                        pre_pearson: pre,
                        post_pearson: post,
                        min_pearson: None,
                        confidence: None,
                        ranking_score: None,
                        failure_stage: Some(stage),
                    }
                }
            }
        })
        .collect()
}

fn scores_from_ctx(ctx: &W5CellContext, cell: W5AnchorRescueCell, wall_ms: u64) -> W5AnchorRescueCellScores {
    W5AnchorRescueCellScores {
        cell,
        nominal_pre: ctx.nominal.0,
        nominal_post: ctx.nominal.1,
        baseline_pre: ctx.baseline_throat.0,
        baseline_post: ctx.baseline_throat.1,
        brackets: score_brackets(ctx),
        wall_ms,
    }
}

/// Score one W5 cell: nominal + baseline throat Pearson, then every feasible anchor bracket on the
/// unified gate path (Phase 1). See module docs and the plan §5.1.
pub fn score_w5_anchor_rescue_cell(cell: W5AnchorRescueCell) -> W5AnchorRescueCellScores {
    let started = Instant::now();
    match build_w5_cell_context(&cell) {
        Some(ctx) => scores_from_ctx(&ctx, cell, started.elapsed().as_millis() as u64),
        None => degenerate_scores(cell, started.elapsed().as_millis() as u64),
    }
}

fn joint_winner_of(joint: &OracleJointOutcome) -> W5JointWinner {
    if !joint.patched {
        W5JointWinner::Skip
    } else if joint.anchor_seam_used && joint.anchor_move_frames > 0 {
        W5JointWinner::Anchor {
            move_frames: joint.anchor_move_frames,
        }
    } else {
        W5JointWinner::Baseline
    }
}

/// Scores for an arbitrary fixture + repair (ad-hoc probes such as the noise-collar variant), not
/// tied to a `(peak_offset, search)` grid cell. `baseline` is `None` if the unified match degenerates.
#[derive(Debug, Clone)]
pub struct W5FixtureScores {
    pub nominal: (f64, f64),
    pub baseline: Option<(f64, f64)>,
    pub brackets: Vec<W5BracketGateScore>,
    pub joint_winner: W5JointWinner,
    pub anchor_seam_would_run: bool,
}

/// Score an arbitrary fixture on the unified gate path: nominal + baseline throat, per-bracket gate
/// scores (with `failure_stage`), production joint winner, and `anchor_seam_would_run`. Used to probe
/// the noise-collar A6 variant (plan §8 Q1).
pub fn score_w5_fixture(
    fixture: &EnergySignatureFixture,
    repair: &RepairConfig,
) -> W5FixtureScores {
    let Some(ctx) = context_from_fixture(fixture, repair) else {
        return W5FixtureScores {
            nominal: (f64::NAN, f64::NAN),
            baseline: None,
            brackets: Vec::new(),
            joint_winner: W5JointWinner::Skip,
            anchor_seam_would_run: false,
        };
    };
    let brackets = score_brackets(&ctx);
    let params = ctx.params();
    let joint_winner = joint_winner_of(&oracle_evaluate_fit_joint(&params, ctx.baseline));
    let anchor_seam_would_run = oracle_anchor_seam_would_run(
        &params,
        ctx.baseline,
        ctx.baseline_throat.0,
        ctx.baseline_throat.1,
    );
    W5FixtureScores {
        nominal: ctx.nominal,
        baseline: Some(ctx.baseline_throat),
        brackets,
        joint_winner,
        anchor_seam_would_run,
    }
}

/// Full Phase 2 evaluation: Phase 1 scores + production joint winner + `anchor_seam_would_run`.
pub fn evaluate_w5_cell(cell: W5AnchorRescueCell) -> W5CellEvaluation {
    let started = Instant::now();
    let ctx = match build_w5_cell_context(&cell) {
        Some(ctx) => ctx,
        None => {
            // Degenerate geometry — record as a skip without running the gate/joint.
            return W5CellEvaluation {
                scores: degenerate_scores(cell, started.elapsed().as_millis() as u64),
                joint_winner: W5JointWinner::Skip,
                anchor_seam_would_run: false,
            };
        }
    };
    let scores = scores_from_ctx(&ctx, cell, started.elapsed().as_millis() as u64);

    let params = ctx.params();
    let joint_winner = joint_winner_of(&oracle_evaluate_fit_joint(&params, ctx.baseline));
    let anchor_seam_would_run = oracle_anchor_seam_would_run(
        &params,
        ctx.baseline,
        ctx.baseline_throat.0,
        ctx.baseline_throat.1,
    );

    W5CellEvaluation {
        scores,
        joint_winner,
        anchor_seam_would_run,
    }
}

/// Classify a cell into a behavioral regime (plan §5.2.3). Rule order is significant.
pub fn classify_w5_cell(eval: &W5CellEvaluation) -> W5AnchorRescueRegime {
    let scores = &eval.scores;
    if scores.cell.is_invalid() {
        return W5AnchorRescueRegime::Invalid;
    }
    if eval.joint_winner == W5JointWinner::Skip {
        return W5AnchorRescueRegime::AllSkip;
    }
    if scores.baseline_min() >= HIGH_FLOOR {
        return W5AnchorRescueRegime::BaselineHigh;
    }
    let anchor_bracket_high = scores
        .max_bracket_min()
        .is_some_and(|m| m >= HIGH_FLOOR);
    let anchor_won = matches!(eval.joint_winner, W5JointWinner::Anchor { .. });
    if anchor_bracket_high && anchor_won {
        return W5AnchorRescueRegime::AnchorRescuePossible;
    }
    W5AnchorRescueRegime::BaselineMarginalWins
}
