//! W5 anchor-rescue single-cell diagnostic (Phase 1).
//! See docs/TEMP-w5-anchor-rescue-diag-plan.md.
//!
//! For one discovery cell `(peak_offset_secs, fill_border_search_secs)` this scores the
//! symmetric-weak-throat fixture's **nominal** and **baseline** throat Pearson plus every feasible
//! anchor bracket on the **unified haystack gate path** — the same path `AudioFitSource::score`
//! drives in production (via [`oracle_score_fit_candidate`]). That is the strict superset of the old
//! `probe_w5_anchor_rescue_scores` probe: it adds per-bracket gate Pearson, confidence and ranking
//! score so we can see whether E3 (anchor rescue) can actually win. Diagnostic tier — emits data,
//! no PR gate; no regime labels (those are Phase 2).

use std::time::Instant;

use clip_sync::MultiChannelPcm;

use crate::application::patch_region::{
    derive_seam_gate_geometry, oracle_build_fit_cache, oracle_score_fit_candidate, SeamGateConfig,
    SeamGateParams,
};
use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamMode, AnchorSeamParams,
};
use crate::domain::FillConfidence;
use crate::infrastructure::config::RepairConfig;
use crate::test_support::energy_signature_fixtures::{
    build_w5_symmetric_weak_throat_anchor_rescue, gap_report_times, EnergySignatureFixture,
};
use crate::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, oracle_baseline_throat_pearson, oracle_nominal_throat_pearson,
    patch_request_from_repair, production_geometry_params,
};
use crate::test_support::patch_geometry_preview::{preview_patch_geometry, slice_b_interleaved};

/// One W5 discovery cell (Phase 1+). `fill_border_search_secs` must be `< peak_offset_secs` for the
/// fixture to be valid (baseline unified search must not High-short-circuit at the throat).
#[derive(Debug, Clone, Copy)]
pub struct W5AnchorRescueCell {
    /// B dropout shift = A peak offset (F1-style).
    pub peak_offset_secs: f64,
    /// Repair + structure `search_radius`; must be `< peak_offset_secs`.
    pub fill_border_search_secs: f64,
}

impl Default for W5AnchorRescueCell {
    fn default() -> Self {
        Self {
            peak_offset_secs: 1.0,
            fill_border_search_secs: 0.78,
        }
    }
}

/// Paired fixture + repair for one cell (`anchor_seam_mode = Auto`).
pub fn build_w5_cell(cell: &W5AnchorRescueCell) -> (EnergySignatureFixture, RepairConfig) {
    let fixture = build_w5_symmetric_weak_throat_anchor_rescue(
        48_000,
        1,
        cell.peak_offset_secs,
        cell.fill_border_search_secs,
    );
    let repair = w5_anchor_rescue_repair_for(cell);
    (fixture, repair)
}

fn w5_anchor_rescue_repair_for(cell: &W5AnchorRescueCell) -> RepairConfig {
    crate::test_support::energy_signature_production::w5_anchor_rescue_repair(
        AnchorSeamMode::Auto,
        cell.fill_border_search_secs,
    )
}

/// Anchor-search params matching the A6 domain oracle
/// (`w5_fixture_throat_symmetric_weak_and_brackets_exist`).
fn anchor_params(fixture: &EnergySignatureFixture) -> AnchorSeamParams {
    AnchorSeamParams {
        context_frames: fixture.context_frames,
        max_anchors_per_side: 5,
        max_bracket_frames: (5.0 * fixture.sample_rate as f64).round() as usize,
        min_prominence: 0.0,
        structure: fixture.structure_params,
    }
}

/// One anchor bracket scored on the unified gate path.
#[derive(Debug, Clone)]
pub struct W5BracketGateScore {
    pub pre_frame: usize,
    pub post_frame: usize,
    pub move_frames: usize,
    pub passed_gate: bool,
    pub pre_pearson: Option<f64>,
    pub post_pearson: Option<f64>,
    /// `min(pre, post)` when the gate passed.
    pub min_pearson: Option<f64>,
    pub confidence: Option<FillConfidence>,
    pub ranking_score: Option<f64>,
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

/// Score one W5 cell: nominal + baseline throat Pearson, then every feasible anchor bracket on the
/// unified gate path. See module docs and the plan §5.1.
pub fn score_w5_anchor_rescue_cell(cell: W5AnchorRescueCell) -> W5AnchorRescueCellScores {
    let started = Instant::now();
    let (fixture, repair) = build_w5_cell(&cell);

    let (nominal_pre, nominal_post) = oracle_nominal_throat_pearson(&fixture, &repair);
    let (baseline_pre, baseline_post) = oracle_baseline_throat_pearson(&fixture, &repair);

    // Production-equivalent per-gap geometry (B window, refined gap) for the haystack gate path.
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let (a_start, a_end, b_start, b_end, _total) = gap_report_times(&fixture);
    let preview = preview_patch_geometry(
        &fixture,
        &report.alignment,
        a_start,
        a_end,
        b_start,
        b_end,
        &production_geometry_params(&repair),
    );
    let silence_peak_fraction = report.silence_peak_fraction;
    let request = patch_request_from_repair(report, &repair);

    let ch = fixture.channels.max(1);
    let cfg = SeamGateConfig::from_repair(
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
    let geom = derive_seam_gate_geometry(
        &cfg,
        &a_pcm,
        &b_haystack,
        preview.b_extract_start_secs,
        preview.refined_b_start_secs,
        preview.refined_b_end_secs,
        gap_frames,
        None,
    );
    let params = SeamGateParams { cfg: &cfg, geom };
    // The haystack cache depends only on `params`; build once and reuse across every bracket.
    let cache = oracle_build_fit_cache(&params);

    // Feasible brackets relative to the gate baseline (same helpers as the A6 domain oracle).
    let aparams = anchor_params(&fixture);
    let candidates = list_anchor_candidates_a(&fixture.a_samples, fixture.channels, baseline, &aparams);
    let brackets = list_feasible_anchor_brackets(&candidates, baseline, &aparams);

    let scored = brackets
        .iter()
        .map(|bracket| match oracle_score_fit_candidate(&params, &cache, bracket.refined, baseline, true) {
            Ok((pre, post, confidence, ranking_score)) => W5BracketGateScore {
                pre_frame: bracket.pre.frame,
                post_frame: bracket.post.frame,
                move_frames: bracket.move_frames,
                passed_gate: true,
                pre_pearson: Some(pre),
                post_pearson: Some(post),
                min_pearson: Some(pre.min(post)),
                confidence: Some(confidence),
                ranking_score: Some(ranking_score),
            },
            Err(_) => W5BracketGateScore {
                pre_frame: bracket.pre.frame,
                post_frame: bracket.post.frame,
                move_frames: bracket.move_frames,
                passed_gate: false,
                pre_pearson: None,
                post_pearson: None,
                min_pearson: None,
                confidence: None,
                ranking_score: None,
            },
        })
        .collect();

    W5AnchorRescueCellScores {
        cell,
        nominal_pre,
        nominal_post,
        baseline_pre,
        baseline_post,
        brackets: scored,
        wall_ms: started.elapsed().as_millis() as u64,
    }
}
