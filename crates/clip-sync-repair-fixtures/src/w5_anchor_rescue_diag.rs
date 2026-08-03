//! W5 seam-gate scoring for an arbitrary energy-signature fixture.
//!
//! [`score_w5_fixture`] puts a fixture through the **unified haystack gate path** — the same path
//! `AudioFitSource::score` drives in production (via [`oracle_score_fit_candidate`]) — and reports
//! the nominal + baseline throat Pearson, every feasible anchor bracket with the stage that rejected
//! it, and the **production-faithful joint winner** (full E1–E7 routing on oracle-built params, no
//! decode). Diagnostic tier — emits data, no PR gate.
//!
//! This began as the `(peak_offset, search)` grid sweep of the anchor-rescue discovery plan
//! (docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md, COMPLETE 2026-06-27). That sweep and its
//! regime classifier are gone; what the plan concluded is now guarded by
//! `clip-sync-repair/tests/anchor_seam_oracle.rs`. The per-fixture scoring below survives because
//! the live timing-offset and quiet-gap diagnostics still ask the same question of other fixtures.
//! Summarizing the brackets is `clip_sync_repair_harness::seam_gate_failures`.

use clip_sync::MultiChannelPcm;

use crate::energy_signature_fixtures::{gap_report_times, EnergySignatureFixture};
use crate::energy_signature_production::{
    gap_report_from_energy_fixture, oracle_baseline_throat_pearson_opt,
    oracle_nominal_throat_pearson, patch_request_from_repair, production_geometry_params,
};
use crate::patch_geometry_preview::{preview_patch_geometry, slice_b_interleaved};
use clip_sync_repair::application::gate_oracle::{
    derive_seam_gate_geometry, oracle_anchor_seam_would_run, oracle_build_fit_cache,
    oracle_evaluate_fit_joint, oracle_score_fit_candidate, OracleJointOutcome, SeamGateDerived,
    SeamGateFailure, SeamGateParams,
};
use clip_sync_repair::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorBracket, AnchorSeamParams,
};
use clip_sync_repair::domain::policies::RefinedGapFrames;
use clip_sync_repair::domain::FillConfidence;
use clip_sync_repair::infrastructure::config::RepairConfig;

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

/// Which candidate won the production joint pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum W5JointWinner {
    /// Whole gate skipped (E5) — nothing patched.
    Skip,
    /// Baseline throat won without engaging the anchor path.
    Baseline,
    /// An editorial anchor bracket won (`anchor_seam_used`), moving `move_frames` from baseline.
    Anchor { move_frames: usize },
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

/// Scoring context for a fixture + repair. `None` for a **degenerate** fixture whose baseline
/// unified haystack match fails (empty extract window / no match) — callers report that as a skip
/// rather than aborting (plan §5.2.8 robustness).
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
    let silence_peak_fraction = report.recipe.silence_peak_fraction();
    let request = patch_request_from_repair(report, repair);

    let ch = fixture.channels.max(1);
    let derived = clip_sync_repair::application::gate_oracle::seam_gate_derived_from_repair(
        &request,
        fixture.sample_rate,
        fixture.channels,
        silence_peak_fraction,
    );
    let a_pcm = pcm_from_samples(
        fixture.a_samples.clone(),
        fixture.sample_rate,
        fixture.channels,
    );
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
