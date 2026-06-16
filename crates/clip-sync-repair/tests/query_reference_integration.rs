//! Query-reference repair integration (Phase Q3).

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{AlignConfig, AlignmentModeUsedReport, ClipConfig, SymphoniaMediaReader};
use clip_sync_repair::application::{PatchAudio, PatchAudioRequest, ScanGaps, ScanGapsRequest};
use clip_sync_repair::domain::{build_gap_fill_plan, GapFillSkipReason, GapPatchStatus};
use clip_sync_repair::infrastructure::aligner::SymphoniaAligner;
use hound::{SampleFormat, WavSpec, WavWriter};

const SAMPLE_RATE: u32 = 11_025;
const A_TOTAL_SECS: u32 = 360;
const QUERY_ANCHOR_SECS: u32 = 240;
const QUERY_DURATION_SECS: u32 = 90;
const GAP_INSIDE_START: u32 = 248;
const GAP_INSIDE_END: u32 = 283;
const GAP_OUTSIDE_START: u32 = 60;
const GAP_OUTSIDE_END: u32 = 90;

fn chirp_sample(sample_rate: u32, index: u64) -> i16 {
    let t = index as f32 / sample_rate as f32;
    let freq = 300.0 + 400.0 * t;
    ((TAU * freq * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
}

fn write_mono_chirp(path: &Path, sample_rate: u32, total_secs: u32) {
    let total_samples = u64::from(sample_rate) * u64::from(total_secs);
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for index in 0..total_samples {
        writer
            .write_sample(chirp_sample(sample_rate, index))
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn write_chirp_slice(path: &Path, sample_rate: u32, anchor_secs: u32, duration_secs: u32) {
    let start_index = u64::from(sample_rate) * u64::from(anchor_secs);
    let count = u64::from(sample_rate) * u64::from(duration_secs);
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for offset in 0..count {
        writer
            .write_sample(chirp_sample(sample_rate, start_index + offset))
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn mute_segment(path: &Path, sample_rate: u32, start_secs: u32, end_secs: u32) {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    let start = (u64::from(sample_rate) * u64::from(start_secs)) as usize;
    let end = (u64::from(sample_rate) * u64::from(end_secs)) as usize;
    let mut muted = samples;
    for sample in muted.iter_mut().take(end).skip(start) {
        *sample = 0;
    }
    let spec = reader.spec();
    let mut writer = WavWriter::create(path, spec).expect("rewrite wav");
    for sample in muted {
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn repair_align_config() -> AlignConfig {
    let mut align = AlignConfig {
        clip: ClipConfig {
            clip_length: Duration::from_secs(60),
            num_clips: 2,
            target_sample_rate: Some(SAMPLE_RATE),
            ..ClipConfig::default()
        },
        ..Default::default()
    };
    align.alignment.refine_offset_high_rate = false;
    align.validation.verify_offset = false;
    align
}

fn scan_request(path_a: PathBuf, path_b: PathBuf) -> ScanGapsRequest {
    ScanGapsRequest {
        video_a: path_a,
        video_b: path_b,
        align: repair_align_config(),
        decode_chunk_secs: 10,
        scan_block_secs: 0.25,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
        silence_hold_blocks: 0,
        min_gap_secs: 25.0,
        scan_both: false,
        gap_offset_tolerance_secs: 0.5,
        limit_fill_to_mapped_region: true,
    }
}

fn write_query_fixture(temp: &Path) -> (PathBuf, PathBuf) {
    let path_a = temp.join("long_a.wav");
    let path_b = temp.join("short_b.wav");
    write_mono_chirp(&path_a, SAMPLE_RATE, A_TOTAL_SECS);
    write_chirp_slice(
        &path_b,
        SAMPLE_RATE,
        QUERY_ANCHOR_SECS,
        QUERY_DURATION_SECS,
    );
    mute_segment(
        &path_a,
        SAMPLE_RATE,
        GAP_INSIDE_START,
        GAP_INSIDE_END,
    );
    mute_segment(
        &path_a,
        SAMPLE_RATE,
        GAP_OUTSIDE_START,
        GAP_OUTSIDE_END,
    );
    (path_a, path_b)
}

#[test]
fn repair_auto_no_clip_count_mismatch_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_query_fixture(temp.path());

    let report = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a, path_b))
    .expect("long A + short B should complete under Auto query mode");

    assert_eq!(
        report.alignment.alignment_mode_used,
        Some(AlignmentModeUsedReport::QueryReference)
    );
    assert!(report.alignment.query_localization.is_some());
    let loc = report
        .alignment
        .query_localization
        .as_ref()
        .expect("localization");
    assert!(
        loc.skip_reason.is_none(),
        "expected successful localization, got skip: {:?}",
        loc.skip_reason
    );
    assert!(
        (loc.anchor_a_secs - f64::from(QUERY_ANCHOR_SECS)).abs() < 2.0,
        "anchor={} expected ~{QUERY_ANCHOR_SECS}",
        loc.anchor_a_secs
    );
}

#[test]
fn repair_query_gap_inside_region_fillable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_query_fixture(temp.path());

    let report = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a.clone(), path_b.clone()))
    .expect("scan");

    let inside_gap = report
        .gaps
        .iter()
        .find(|g| {
            (g.video_a_start_secs - f64::from(GAP_INSIDE_START)).abs() < 5.0
                && (g.video_a_end_secs - f64::from(GAP_INSIDE_END)).abs() < 5.0
        })
        .unwrap_or_else(|| {
            panic!(
                "gap inside mapped region not found; gaps={:?}",
                report
                    .gaps
                    .iter()
                    .map(|g| (g.video_a_start_secs, g.video_a_end_secs))
                    .collect::<Vec<_>>()
            );
        });
    assert!(inside_gap.is_fillable());
    assert!(
        !report.gap_outside_reference_coverage(inside_gap),
        "inside gap [{}, {}] should lie within mapped region {:?}",
        inside_gap.video_a_start_secs,
        inside_gap.video_a_end_secs,
        report.overlap
    );

    let plan = build_gap_fill_plan(&report, 10, true);
    assert!(
        plan.regions
            .iter()
            .any(|r| (r.a_start_secs - f64::from(GAP_INSIDE_START)).abs() < 5.0),
        "inside gap should be in fill plan"
    );

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(
            PatchAudioRequest {
                report,
                normalize_fill: false,
                normalize_window_secs: 5.0,
                max_fill_gain_db: 12.0,
                min_fill_correlation: 0.35,
                fill_align_margin_secs: 1.0,
                max_fill_align_adjustment_secs: 0.5,
                fill_border_search_secs: 30.0,
                min_border_discovery_secs: 2.0,
                border_standoff_secs: 0.35,
                short_gap_mean_correlation_secs: 2.0,
                fill_length_slack_secs: 5.0,
                fill_seam_search_secs: 0.25,
                gap_signature_context_secs: 3.0,
                gap_signature_bin_ms: 50,
                min_structure_match_score: 0.55,
                strong_structure_trust: 0.90,
                partial_structure_waveform_soften: 0.85,
                absolute_silence_rms: 0.0,
                limit_fill_to_mapped_region: true,
            },
            10,
        )
        .expect("patch inside-region gap");

    let patched = result
        .summary
        .gaps
        .iter()
        .find(|g| (g.a_start_secs - f64::from(GAP_INSIDE_START)).abs() < 5.0)
        .expect("inside gap outcome");
    assert!(
        matches!(patched.status, GapPatchStatus::Patched { .. }),
        "expected patched, got {:?}",
        patched.status
    );
}

#[test]
fn repair_query_gap_outside_region_skipped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_query_fixture(temp.path());

    let report = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a, path_b))
    .expect("scan");

    let outside_gap = report
        .gaps
        .iter()
        .find(|g| {
            (g.video_a_start_secs - f64::from(GAP_OUTSIDE_START)).abs() < 5.0
                && (g.video_a_end_secs - f64::from(GAP_OUTSIDE_END)).abs() < 5.0
        })
        .expect("gap outside mapped region should still be reported");
    assert!(report.gap_outside_reference_coverage(outside_gap));

    let plan = build_gap_fill_plan(&report, 10, true);
    assert!(
        !plan
            .regions
            .iter()
            .any(|r| (r.a_start_secs - f64::from(GAP_OUTSIDE_START)).abs() < 5.0),
        "outside gap must not appear in fill regions"
    );
    if outside_gap.is_fillable() {
        assert!(
            plan.skipped.iter().any(|s| {
                (s.a_start_secs - f64::from(GAP_OUTSIDE_START)).abs() < 5.0
                    && s.reason == GapFillSkipReason::OutsideReferenceCoverage
            }),
            "fillable outside gap should be skipped with OutsideReferenceCoverage"
        );
    }
}
