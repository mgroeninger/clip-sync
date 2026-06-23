//! Production-scale energy corpus helpers (`docs/TEMP-energy-corpus-plan.md`).

use std::path::Path;

use clip_sync::{
    AlignVideosRequest, AlignmentResult, ClipLabel, ClipMatch, ProgressReporter,
    SymphoniaMediaReader, TimelineOverlap,
};

use crate::test_support::NoOpProgressReporter;

use crate::application::ports::Aligner;
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::application::PatchAudioRequest;
use crate::domain::{GapReport, GapSignatureMode};
use crate::infrastructure::config::RepairConfig;
use crate::test_support::energy_signature_fixtures::{
    gap_report_times, write_fixture_wavs, EnergySignatureFixture,
};

/// Minimum fixture length for a valid production patch matrix context (lead-in + gap + tail).
///
/// Layout: `anchor + min_gap + context + border + margin` where
/// `anchor = context + border + margin` (see `gap_anchor_secs`).
pub fn min_total_secs_for_signature_context(context_secs: f64) -> f64 {
    const BORDER: f64 = 10.0;
    const MARGIN: f64 = 1.0;
    const MIN_GAP: f64 = 1.0;
    let anchor = context_secs + BORDER + MARGIN;
    anchor + MIN_GAP + context_secs + BORDER + MARGIN
}

/// Production matrix contexts valid for a fixture of `total_secs` (skips e.g. context 30 @ 60 s).
pub fn production_matrix_contexts(total_secs: f64) -> Vec<f64> {
    [3.0, 10.0, 30.0]
        .into_iter()
        .filter(|&context| total_secs + f64::EPSILON >= min_total_secs_for_signature_context(context))
        .collect()
}

pub fn gap_report_from_energy_fixture(
    temp: &Path,
    fixture: &EnergySignatureFixture,
) -> GapReport {
    use crate::domain::gap::Gap;

    let (path_a, path_b) = write_fixture_wavs(temp, fixture);
    let (a_start, a_end, b_start, b_end, total_secs) = gap_report_times(fixture);
    GapReport {
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(crate::domain::TrackCompatibility {
            a_channels: fixture.channels as u16,
            b_channels: fixture.channels as u16,
            a_sample_rate: fixture.sample_rate,
            b_sample_rate: fixture.sample_rate,
            channels_match: true,
            rate_match: true,
            verdict: crate::domain::CompatibilityVerdict::Compatible,
        }),
        alignment: oracle_injected_alignment(total_secs),
        gaps: vec![Gap {
            video_a_start_secs: a_start,
            video_a_end_secs: a_end,
            video_b_start_secs: Some(b_start),
            video_b_end_secs: Some(b_end),
            b_has_energy: true,
        }],
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        scan_block_ms: 250,
        silence_peak_fraction: 0.01,
        limit_fill_to_mapped_region: true,
        audio_timeline_skew: None,
    }
}

/// Repair config mirroring production defaults for signature matrix runs.
pub fn production_repair_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
) -> RepairConfig {
    RepairConfig {
        gap_signature_mode,
        gap_signature_context_secs,
        fill_border_search_secs: 10.0,
        fill_align_margin_secs: 1.0,
        gap_end_extend_on_post_seam_fail: true,
        gap_start_extend_on_pre_seam_fail: true,
        ..Default::default()
    }
}

struct NeverCalledAligner;

impl Aligner for NeverCalledAligner {
    fn align(
        &self,
        _: AlignVideosRequest,
        _: &dyn ProgressReporter,
    ) -> Result<AlignmentResult, clip_sync::AppError> {
        unreachable!("energy production corpus uses scan_after_alignment directly")
    }
}

/// Scan path: single start clip + overlap (aligner not invoked).
fn zero_offset_alignment(duration_secs: f64) -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: duration_secs,
            aligned: true,
            offset_secs: Some(0.0),
            confidence: 0.95,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
            video_b_window_start_secs: None,
            video_b_window_end_secs: None,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(0.0),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: Some(TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: duration_secs,
            video_b_start_secs: 0.0,
            video_b_end_secs: duration_secs,
            shared_length_secs: duration_secs,
        }),
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}

/// I1-style injected report: zero drift, start + end clips (matches integration oracle).
fn oracle_injected_alignment(timeline_secs: f64) -> AlignmentResult {
    let half = timeline_secs / 2.0;
    AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: half,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: half,
                window_end_secs: timeline_secs,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some(0.0),
        offsets_consistent: false,
        offset_drift_secs: Some(0.0),
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}

/// Scan A/B WAVs written from a synthetic fixture (production scan defaults).
pub fn scan_gaps_for_fixture(fixture: &EnergySignatureFixture, temp: &Path) -> GapReport {
    let (path_a, path_b) = write_fixture_wavs(temp, fixture);
    let (_, _, _, _, total_secs) = gap_report_times(fixture);
    let repair = RepairConfig::default();
    let request = ScanGapsRequest {
        video_a: path_a,
        video_b: path_b,
        align: Default::default(),
        decode_chunk_secs: repair.decode_chunk_secs,
        scan_block_secs: repair.scan_block_secs(),
        silence_peak_fraction: repair.silence_peak_fraction,
        absolute_silence_rms: repair.absolute_silence_rms,
        silence_hold_blocks: repair.silence_hold_blocks(),
        min_gap_secs: repair.min_gap_secs(),
        scan_both: false,
        gap_offset_tolerance_secs: repair.gap_offset_tolerance_secs,
        limit_fill_to_mapped_region: true,
    };
    let media_reader = SymphoniaMediaReader;
    let progress = NoOpProgressReporter;
    let scan = ScanGaps::new(&media_reader, &progress, &NeverCalledAligner);
    scan.scan_after_alignment(request, zero_offset_alignment(total_secs))
        .expect("scan energy fixture WAV")
}

pub fn patch_request_from_repair(report: GapReport, repair: &RepairConfig) -> PatchAudioRequest {
    repair.patch_settings().into_request(report)
}

/// Patch geometry params mirroring [`production_repair_config`] for haystack diagnostics.
pub fn production_geometry_params(repair: &RepairConfig) -> crate::test_support::patch_geometry_preview::PatchGeometryParams {
    use crate::domain::FillMode;
    use crate::test_support::patch_geometry_preview::PatchGeometryParams;

    PatchGeometryParams {
        fill_border_search_secs: repair.fill_border_search_secs,
        fill_align_margin_secs: repair.fill_align_margin_secs,
        gap_signature_context_secs: repair.gap_signature_context_secs,
        fill_length_slack_secs: repair.fill_length_slack_secs,
        gap_end_extend_max_ms: repair.gap_end_extend_max_ms,
        gap_end_extend_on_post_seam_fail: repair.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: repair.gap_start_extend_on_pre_seam_fail,
        fit_boundary_search: repair.fit_boundary_search,
        fill_offset_mode: repair.fill_offset_mode,
        fill_mode_fit: repair.fill_mode == FillMode::Fit,
        gap_signature_bin_ms: repair.gap_signature_bin_ms as u32,
    }
}

/// Replace scan alignment with I1-style oracle injection (start + end clips, zero drift).
pub fn inject_oracle_alignment(report: &mut GapReport, total_secs: f64) {
    report.alignment = oracle_injected_alignment(total_secs);
}

/// After scan, remap B gap bounds using fixture nominal refine (matches [`gap_report_times`] B leg).
pub fn normalize_scan_gap_b_mapping(report: &mut GapReport, fixture: &EnergySignatureFixture) {
    let (_, _, b_start, b_end, total_secs) = gap_report_times(fixture);
    inject_oracle_alignment(report, total_secs);
    for gap in &mut report.gaps {
        if gap.is_fillable() {
            gap.video_b_start_secs = Some(b_start);
            gap.video_b_end_secs = Some(b_end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::energy_signature_fixtures::build_f1_production;

    #[test]
    fn scan_detects_f1_production_gap() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = build_f1_production(48_000, 2, 3.0);
        let (expected_start, expected_end, _, _, _) = gap_report_times(&fixture);
        let report = scan_gaps_for_fixture(&fixture, temp.path());
        let fillable: Vec<_> = report.gaps.iter().filter(|g| g.is_fillable()).collect();
        assert_eq!(fillable.len(), 1, "expected one fillable gap: {:#?}", report.gaps);
        let gap = fillable[0];
        const TOL: f64 = 0.35;
        assert!(
            (gap.video_a_start_secs - expected_start).abs() <= TOL,
            "scan start {:.3} expected {:.3}",
            gap.video_a_start_secs,
            expected_start,
        );
        assert!(
            (gap.video_a_end_secs - expected_end).abs() <= TOL,
            "scan end {:.3} expected {:.3}",
            gap.video_a_end_secs,
            expected_end,
        );
    }

    use crate::domain::GapSignatureMode;
    use crate::test_support::energy_signature_fixtures::structure_heavy_weights;
    use crate::test_support::patch_geometry_preview::preview_patch_geometry;

    #[test]
    fn production_matrix_contexts_skip_thirty_on_sixty_sec_fixture() {
        let contexts = production_matrix_contexts(60.0);
        assert_eq!(contexts, vec![3.0, 10.0]);
        assert_eq!(production_matrix_contexts(120.0), vec![3.0, 10.0, 30.0]);
    }

    #[test]
    fn f1_production_scan_and_domain_smoke() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = build_f1_production(48_000, 2, 3.0);
        let report = scan_gaps_for_fixture(&fixture, temp.path());
        assert_eq!(
            report.gaps.iter().filter(|g| g.is_fillable()).count(),
            1,
            "scan should find one fillable gap: {:#?}",
            report.gaps,
        );

        let matched = fixture
            .unified_match(GapSignatureMode::Auto, structure_heavy_weights())
            .expect("F1-long auto domain on full B");
        assert!(
            fixture.within_bin_tolerance(matched.alignment.start_frame, fixture.true_fill_start),
            "auto domain start {} true {}",
            matched.alignment.start_frame,
            fixture.true_fill_start,
        );
    }

    #[test]
    fn f1_production_haystack_scan_vs_oracle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fixture = build_f1_production(48_000, 2, 3.0);
        let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
        let params = production_geometry_params(&repair);
        let weights = structure_heavy_weights();

        let scan_report = scan_gaps_for_fixture(&fixture, temp.path());
        let scan_gap = scan_report
            .gaps
            .iter()
            .find(|g| g.is_fillable())
            .expect("fillable scan gap");
        let scan_preview = preview_patch_geometry(
            &fixture,
            &scan_report.alignment,
            scan_gap.video_a_start_secs,
            scan_gap.video_a_end_secs,
            scan_gap.video_b_start_secs.unwrap_or(0.0),
            scan_gap.video_b_end_secs.unwrap_or(0.0),
            &params,
        );

        let (oracle_a_start, oracle_a_end, oracle_b_start, oracle_b_end, _) =
            gap_report_times(&fixture);
        let oracle_report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let oracle_preview = preview_patch_geometry(
            &fixture,
            &oracle_report.alignment,
            oracle_a_start,
            oracle_a_end,
            oracle_b_start,
            oracle_b_end,
            &params,
        );

        eprintln!("{}", scan_preview.format_diagnostic(&fixture));
        eprintln!("{}", oracle_preview.format_diagnostic(&fixture));

        let scan_haystack = scan_preview
            .unified_match_on_haystack(&fixture, GapSignatureMode::Energy, weights);
        let oracle_haystack = oracle_preview
            .unified_match_on_haystack(&fixture, GapSignatureMode::Energy, weights);

        assert!(
            oracle_preview.true_within_search_radius,
            "oracle control: true fill must be within search radius"
        );
        assert!(
            oracle_haystack.is_some(),
            "oracle haystack unified match should succeed"
        );

        if !scan_preview.true_within_search_radius {
            eprintln!("scan path: true fill outside search radius (expected blocker)");
        }
        if scan_haystack.is_none() {
            eprintln!("scan path: haystack unified match failed");
        }
    }
}
