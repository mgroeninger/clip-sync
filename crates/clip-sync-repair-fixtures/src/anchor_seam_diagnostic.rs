//! Per-gap anchor seam candidate / bracket dumps for diagnostic tests.

use clip_sync_repair::domain::gap_anchor_seam::{
    anchor_bracket_both_matchable, list_anchor_candidates_a, list_feasible_anchor_brackets,
    matchability_at_anchor, AnchorMatchabilityParams, AnchorSeamParams, AnchorSeamSide,
    AnchorSource, DEFAULT_ANCHOR_MATCH_MIN_PEARSON, DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK,
    DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND,
};
use clip_sync_repair::domain::pcm::{interleaved_to_channels, interleaved_to_mono};
use clip_sync_repair::domain::policies::{self, refine_gap_frames, RefinedGapFrames, SeamTemplates};

use super::energy_signature_fixtures::EnergySignatureFixture;

fn refined_scan_hole(fixture: &EnergySignatureFixture) -> RefinedGapFrames {
    let ch = fixture.channels.max(1);
    refine_gap_frames(
        &fixture.a_samples,
        ch,
        fixture.gap_start,
        fixture.gap_end,
        0.01,
        0.0,
        (0.75 * fixture.sample_rate as f64).round() as usize,
    )
}

fn anchor_params(fixture: &EnergySignatureFixture) -> AnchorSeamParams {
    AnchorSeamParams {
        context_frames: fixture.context_frames,
        max_anchors_per_side: 5,
        max_bracket_frames: (5.0 * fixture.sample_rate as f64).round() as usize,
        min_prominence: 0.0,
        structure: fixture.structure_params,
    }
}

fn source_label(source: AnchorSource) -> &'static str {
    match source {
        AnchorSource::ScanRefined => "scan",
        AnchorSource::EnergyPeak => "energy_peak",
        AnchorSource::BoolTransition => "bool_transition",
    }
}

/// CSV header for [`print_anchor_seam_diagnostic`].
pub const ANCHOR_SEAM_DIAG_HEADER: &str = "fixture,scan_start,scan_end,record,side,frame,source,prominence,rms,\
bracket_pre,bracket_post,bracket_move,pearson_pre,pearson_post,xcorr_pre,xcorr_post,matchable";

/// Print anchor candidates, feasible brackets, and per-anchor matchability (stdout).
pub fn print_anchor_seam_diagnostic(fixture: &EnergySignatureFixture, label: &str) {
    let ch = fixture.channels.max(1);
    let scan = refined_scan_hole(fixture);
    let params = anchor_params(fixture);
    let set = list_anchor_candidates_a(&fixture.a_samples, ch, scan, &params);
    let brackets = list_feasible_anchor_brackets(&set, scan, &params);

    println!("{ANCHOR_SEAM_DIAG_HEADER}");

    for side in [AnchorSeamSide::Pre, AnchorSeamSide::Post] {
        let candidates = match side {
            AnchorSeamSide::Pre => &set.pre,
            AnchorSeamSide::Post => &set.post,
        };
        for c in candidates {
            println!(
                "{label},{},{},candidate,{},{},{},{:.4},{:.4},,,,,,,,",
                scan.start_frame,
                scan.end_frame,
                match side {
                    AnchorSeamSide::Pre => "pre",
                    AnchorSeamSide::Post => "post",
                },
                c.frame,
                source_label(c.source),
                c.prominence,
                c.rms,
            );
        }
    }

    let match_params = AnchorMatchabilityParams {
        min_pearson: DEFAULT_ANCHOR_MATCH_MIN_PEARSON,
        min_xcorr_peak: DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK,
        xcorr_ambiguous_band: DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND,
    };
    let correlator = clip_sync_repair::infrastructure::correlation::FftCorrelator::new();
    let max_lag = 200;

    for bracket in &brackets {
        let gap_frames = bracket.refined.end_frame - bracket.refined.start_frame;
        let border_spec = policies::GapBorderSpec {
            gap_start_frame: bracket.refined.start_frame,
            gap_end_frame: bracket.refined.end_frame,
            border_frames: fixture.structure_params.bin_frames * 3,
            border_standoff_frames: 0,
            silence_peak_fraction: fixture.structure_params.silence_peak_fraction,
            absolute_rms_floor: fixture.structure_params.absolute_silence_rms,
        };
        let (a_pre, a_post) =
            policies::border_templates_for_gap(&fixture.a_samples, ch, &border_spec);
        let (a_pre_ch, a_post_ch) =
            policies::border_templates_per_channel_for_gap(&fixture.a_samples, ch, &border_spec);
        let b_mono = interleaved_to_mono(&fixture.b_samples, ch);
        let b_ch = interleaved_to_channels(&fixture.b_samples, ch);
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let pre_w = a_pre.len().max(1);
        let post_w = a_post.len().max(1);
        let placement = policies::SeamPlacement {
            start: fixture.nominal_fill_start,
            gap_frames,
            pre_window: pre_w,
            post_window: post_w,
        };
        let pre_m = matchability_at_anchor(&clip_sync_repair::domain::gap_anchor_seam::MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Pre,
            pre_window: pre_w,
            post_window: post_w,
            params: &match_params,
            correlator: Some(&correlator),
            max_lag_frames: max_lag,
        });
        let post_m = matchability_at_anchor(&clip_sync_repair::domain::gap_anchor_seam::MatchabilityAtAnchorArgs {
            templates: &templates,
            placement,
            side: AnchorSeamSide::Post,
            pre_window: pre_w,
            post_window: post_w,
            params: &match_params,
            correlator: Some(&correlator),
            max_lag_frames: max_lag,
        });
        let matchable = anchor_bracket_both_matchable(
            &templates,
            placement,
            pre_w,
            post_w,
            &match_params,
            Some(&correlator),
            max_lag,
        );
        let xcorr_pre = pre_m
            .xcorr_peak
            .map(|p| format!("{p:.4}"))
            .unwrap_or_else(|| "".into());
        let xcorr_post = post_m
            .xcorr_peak
            .map(|p| format!("{p:.4}"))
            .unwrap_or_else(|| "".into());
        println!(
            "{label},{},{},bracket,,,,,,{},{},{},{:.4},{:.4},{xcorr_pre},{xcorr_post},{matchable}",
            scan.start_frame,
            scan.end_frame,
            bracket.pre.frame,
            bracket.post.frame,
            bracket.move_frames,
            pre_m.pearson,
            post_m.pearson,
        );
    }
}
