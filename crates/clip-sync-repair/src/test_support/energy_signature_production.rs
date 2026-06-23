//! Production-scale energy corpus helpers (`docs/TEMP-energy-corpus-plan.md`).

use std::path::Path;

use clip_sync::{
    AlignVideosRequest, AlignmentResult, ClipLabel, ClipMatch, ProgressReporter,
    SymphoniaMediaReader, TimelineOverlap,
};
use clip_sync::testing::fakes::FakeProgressReporter;

use crate::application::ports::Aligner;
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::application::PatchAudioRequest;
use crate::domain::{GapReport, GapSignatureMode};
use crate::infrastructure::config::RepairConfig;
use crate::test_support::energy_signature_fixtures::{
    gap_report_times, write_fixture_wavs, EnergySignatureFixture,
};

/// Repair config for production-geometry patch smoke (structure-heavy; integration-style
/// border/margin so synthetic F1-long seams patch like I1–I3).
pub fn production_structure_smoke_config(
    gap_signature_mode: GapSignatureMode,
    gap_signature_context_secs: f64,
) -> RepairConfig {
    RepairConfig {
        gap_signature_mode,
        gap_signature_context_secs,
        fill_border_search_secs: 10.0,
        max_fill_align_adjustment_secs: 2.0,
        fill_align_margin_secs: 0.2,
        fill_length_slack_secs: 0.05,
        gap_end_extend_on_post_seam_fail: false,
        gap_start_extend_on_pre_seam_fail: false,
        fill_fit_structure_weight: 1.0,
        fill_fit_waveform_weight: 0.0,
        fill_marginal_margin: 0.08,
        fill_absolute_floor: -0.05,
        min_structure_match_score: 0.0,
        min_border_discovery_secs: 0.25,
        short_gap_one_strong_seam_fallback: true,
        ..Default::default()
    }
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
        alignment: zero_offset_alignment(total_secs),
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
    let progress = FakeProgressReporter;
    let scan = ScanGaps::new(&media_reader, &progress, &NeverCalledAligner);
    scan.scan_after_alignment(request, zero_offset_alignment(total_secs))
        .expect("scan energy fixture WAV")
}

pub fn patch_request_from_repair(report: GapReport, repair: &RepairConfig) -> PatchAudioRequest {
    repair.patch_settings().into_request(report)
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

    use crate::test_support::energy_signature_fixtures::structure_heavy_weights;

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
}
