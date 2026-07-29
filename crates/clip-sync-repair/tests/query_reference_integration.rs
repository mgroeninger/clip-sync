//! Query-reference repair integration.
//!
//! Tier: **integration**. Short chirp pairs in query-reference alignment mode; gaps inside vs
//! outside the mapped B region under `fill_mode = gate`.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test query_reference_integration`

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{AlignConfig, AlignmentModeUsed, ClipConfig, SymphoniaMediaReader};
use clip_sync_repair::application::{PatchAudio, PatchRequestSettings, ScanGaps, ScanGapsRequest};
use clip_sync_repair::domain::{
    build_gap_fill_plan, GapFillSkipReason, GapPatchStatus, GapSelection,
};
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
/// Inside gap on short A's timeline when B is the long donor (mirrors A-long layout).
const GAP_INSIDE_A_START: u32 = GAP_INSIDE_START - QUERY_ANCHOR_SECS;
const GAP_INSIDE_A_END: u32 = GAP_INSIDE_END - QUERY_ANCHOR_SECS;

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
    write_chirp_slice(&path_b, SAMPLE_RATE, QUERY_ANCHOR_SECS, QUERY_DURATION_SECS);
    mute_segment(&path_a, SAMPLE_RATE, GAP_INSIDE_START, GAP_INSIDE_END);
    mute_segment(&path_a, SAMPLE_RATE, GAP_OUTSIDE_START, GAP_OUTSIDE_END);
    (path_a, path_b)
}

fn write_b_longer_query_fixture(temp: &Path) -> (PathBuf, PathBuf) {
    let path_a = temp.join("short_a.wav");
    let path_b = temp.join("long_b.wav");
    write_mono_chirp(&path_b, SAMPLE_RATE, A_TOTAL_SECS);
    write_chirp_slice(&path_a, SAMPLE_RATE, QUERY_ANCHOR_SECS, QUERY_DURATION_SECS);
    mute_segment(&path_a, SAMPLE_RATE, GAP_INSIDE_A_START, GAP_INSIDE_A_END);
    (path_a, path_b)
}

fn mono_region(
    samples: &[f32],
    channels: u16,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> Vec<f32> {
    let ch = usize::from(channels.max(1));
    let start = (start_secs * f64::from(sample_rate)).round() as usize * ch;
    let end = (end_secs * f64::from(sample_rate)).round() as usize * ch;
    samples[start..end.min(samples.len())]
        .iter()
        .step_by(ch)
        .copied()
        .collect()
}

fn patch_inside_gap(
    report: clip_sync_repair::domain::GapReport,
) -> clip_sync_repair::application::PatchAudioResult {
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    // Seeded from production (`..RepairConfig::default().patch_settings()`), same structural
    // pattern as harness `patch_request_with_options` (config-bundles plan P3 step 1;
    // `docs/dev/archive/TEMP-repair-config-bundles-plan.md`). Overrides below
    // are deliberate for this chirp / gate-mode scenario; everything else inherits production.
    // Value-identical to the previous hand-written literal: each absorbed field already matched
    // its production default.
    patch
        .execute(
            PatchRequestSettings {
                // Chirp fixture / gate-path scenario (module docs: fill_mode = gate).
                normalize_fill: false,
                fill_mode: clip_sync_repair::domain::FillMode::Gate,
                gap_signature_mode: clip_sync_repair::domain::GapSignatureMode::Bool,
                fill_border_search_secs: 30.0,
                fill_repeat_penalty_weight: 0.0,
                // Synthetic audio: digital silence, no dedupe/veto/anchor rescue, energy bias
                // inert under Gate (mirrors nominal rather than production's 0.25).
                absolute_silence_rms: 0.0,
                skip_equivalent_gaps: false,
                residual_gate: clip_sync_repair::domain::ResidualGateMode::Off,
                anchor_seam_mode: clip_sync_repair::domain::AnchorSeamMode::Off,
                dual_fit: false,
                fill_fit_energy_nominal_bias_scale: 1.0,
                ..clip_sync_repair::infrastructure::config::RepairConfig::default().patch_settings()
            }
            .into_request(report),
            10,
        )
        .expect("patch inside-region gap")
}

#[test]
fn repair_auto_no_clip_count_mismatch_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_query_fixture(temp.path());

    let outcome = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a, path_b))
    .expect("long A + short B should complete under Auto query mode");

    assert_eq!(
        outcome.alignment_detail.alignment_mode_used,
        Some(AlignmentModeUsed::QueryReference)
    );
    assert!(outcome.alignment_detail.query_localization.is_some());
    let loc = outcome
        .alignment_detail
        .query_localization
        .as_ref()
        .expect("localization");
    assert!(
        loc.skip_reason.is_none(),
        "expected successful localization, got skip: {:?}",
        loc.skip_reason
    );
    assert!(
        (loc.anchor_ref_secs - f64::from(QUERY_ANCHOR_SECS)).abs() < 2.0,
        "anchor={} expected ~{QUERY_ANCHOR_SECS}",
        loc.anchor_ref_secs
    );
}

#[test]
fn repair_query_gap_inside_region_fillable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_query_fixture(temp.path());

    let outcome = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a.clone(), path_b.clone()))
    .expect("scan");
    let report = &*outcome;

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
        report.alignment.start_overlap
    );

    let plan = build_gap_fill_plan(report, 10, false, &GapSelection::all(report.gaps.len()));
    assert!(
        plan.regions
            .iter()
            .any(|r| (r.a_start_secs - f64::from(GAP_INSIDE_START)).abs() < 5.0),
        "inside gap should be in fill plan"
    );

    let result = patch_inside_gap(outcome.report.clone());
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
fn repair_b_longer_query_gap_inside_region_patched_with_donor_audio() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) = write_b_longer_query_fixture(temp.path());

    let outcome = ScanGaps::new(
        &SymphoniaMediaReader,
        &FakeProgressReporter,
        &SymphoniaAligner,
    )
    .execute(scan_request(path_a.clone(), path_b.clone()))
    .expect("short A + long B should complete under Auto query mode");
    let report = &*outcome;

    assert_eq!(
        outcome.alignment_detail.alignment_mode_used,
        Some(AlignmentModeUsed::QueryReference)
    );
    let offset = report
        .alignment
        .recommended_offset_secs
        .expect("recommended offset");
    assert!(
        offset > 0.0,
        "B-longer query mode should yield positive offset, got {offset}"
    );
    assert!(
        (offset - f64::from(QUERY_ANCHOR_SECS)).abs() < 2.0,
        "offset {offset} expected ~{QUERY_ANCHOR_SECS}"
    );
    let loc = outcome
        .alignment_detail
        .query_localization
        .as_ref()
        .expect("localization");
    assert!(loc.skip_reason.is_none());
    assert!(
        loc.clip_on_a_start_secs.abs() < 2.0,
        "short A clip should start near 0, got {}",
        loc.clip_on_a_start_secs
    );

    let inside_gap = report
        .gaps
        .iter()
        .find(|g| {
            (g.video_a_start_secs - f64::from(GAP_INSIDE_A_START)).abs() < 5.0
                && (g.video_a_end_secs - f64::from(GAP_INSIDE_A_END)).abs() < 5.0
        })
        .expect("inside gap on short A");
    assert!(inside_gap.is_fillable());
    assert!(!report.gap_outside_reference_coverage(inside_gap));

    let gap_a_start = inside_gap.video_a_start_secs;
    let gap_a_end = inside_gap.video_a_end_secs;
    let gap_b_start = inside_gap.video_b_start_secs;
    let result = patch_inside_gap(outcome.report.clone());
    let patched = result
        .summary
        .gaps
        .iter()
        .find(|g| (g.a_start_secs - gap_a_start).abs() < 5.0)
        .expect("inside gap outcome");
    assert!(
        matches!(patched.status, GapPatchStatus::Patched { .. }),
        "expected patched, got {:?}",
        patched.status
    );

    let GapPatchStatus::Patched {
        post_correlation,
        pre_correlation,
        ..
    } = &patched.status
    else {
        panic!("expected patched, got {:?}", patched.status);
    };
    assert!(
        *post_correlation >= 0.35,
        "patch post-correlation {post_correlation} below min_fill_correlation gate (pre={pre_correlation})"
    );
    assert!(
        *post_correlation > 0.85,
        "patched gap should correlate strongly with donor chirp, post_correlation={post_correlation}"
    );

    let pcm = result.pcm.expect("patched pcm");
    let interior_start = gap_a_start + 2.0;
    let interior_end = gap_a_end - 2.0;
    assert!(
        interior_end > interior_start,
        "gap too short for interior audio check"
    );
    let filled = mono_region(
        &pcm.samples,
        pcm.channels,
        pcm.sample_rate,
        interior_start,
        interior_end,
    );
    let filled_rms: f64 =
        filled.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / filled.len().max(1) as f64;
    assert!(
        filled_rms.sqrt() > 500.0 / 32767.0,
        "patched gap interior should contain donor audio, rms={}",
        filled_rms.sqrt()
    );
    assert!(
        gap_b_start.expect("b start") >= 0.0,
        "gap_b = gap_a + offset must be non-negative"
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

    let plan = build_gap_fill_plan(&report, 10, false, &GapSelection::all(report.gaps.len()));
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
