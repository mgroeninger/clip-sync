//! Gap scan integration on synthetic chirp/silence WAVs.
//!
//! Tier: **integration**. Exercises `ScanGaps` + aligner wiring (mono chirp pairs, zeroed silence
//! regions). Optional AC-3 dual-track row behind `ac3` + `ffmpeg-tests`.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test scan_gaps_integration`

use std::path::Path;
use std::time::Duration;

use clip_sync::testing::audio_fixtures::write_offset_chirp_wav_pair;
use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::ClipConfig;
use clip_sync::{AlignConfig, SymphoniaMediaReader};
use clip_sync_repair::application::{ScanGaps, ScanGapsRequest};
use clip_sync_repair::domain::b_stall::BStallEvidence;
use clip_sync_repair::infrastructure::aligner::SymphoniaAligner;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
use clip_sync::testing::ffmpeg_util;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECS: u32 = 120;
const OFFSET_SECS: u32 = 3;
const SILENT_START_SECS: u32 = 60;
const SILENT_END_SECS: u32 = 90;

fn zero_wav_segment(path: &Path, sample_rate: u32, start_secs: u32, end_secs: u32) {
    let mut reader = WavReader::open(path).expect("open wav");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|sample| sample.expect("read sample"))
        .collect();

    let start = u64::from(sample_rate) * u64::from(start_secs);
    let end = u64::from(sample_rate) * u64::from(end_secs);
    let mut muted = samples;
    for sample in muted.iter_mut().take(end as usize).skip(start as usize) {
        *sample = 0;
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for sample in muted {
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn aligned_chirp_config() -> AlignConfig {
    AlignConfig {
        clip: ClipConfig {
            clip_length: Duration::from_secs(60),
            num_clips: 1,
            target_sample_rate: Some(SAMPLE_RATE),
            ..ClipConfig::default()
        },
        ..Default::default()
    }
}

#[test]
fn execute_detects_silent_gap_through_alignment_and_scan_pipeline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let scan = ScanGaps::new(&media_reader, &progress, &aligner);

    let report = scan
        .execute(ScanGapsRequest {
            video_a: path_a,
            video_b: path_b,
            align: aligned_chirp_config(),
            decode_chunk_secs: 10,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(
                25_000, 0, 250, 0.01, 0.0,
            ),
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 10.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("full execute path should succeed");

    let offset = report
        .alignment
        .recommended_offset_secs
        .expect("alignment should produce an offset");
    assert!(
        (offset - f64::from(OFFSET_SECS)).abs() < 1.0,
        "offset={offset}, expected about +{OFFSET_SECS}"
    );

    assert!(
        !report.gaps.is_empty(),
        "expected at least one silent gap in video A"
    );

    let silent_gap = report
        .gaps
        .iter()
        .find(|gap| {
            (gap.video_a_start_secs - f64::from(SILENT_START_SECS)).abs() < 1.0
                && (gap.video_a_end_secs - f64::from(SILENT_END_SECS)).abs() < 1.0
        })
        .expect("expected a gap covering the muted A segment");

    assert!(
        silent_gap.b_has_energy,
        "video B should have audio at the aligned position"
    );
    assert!(silent_gap.is_fillable());
    assert!(report.fillable_count() >= 1);
}

#[test]
fn execute_detects_short_two_second_gap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    // Mute a 2-second window in A — the minimum realistic gap we care about detecting.
    zero_wav_segment(&path_a, SAMPLE_RATE, 40, 42);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let scan = ScanGaps::new(&media_reader, &progress, &aligner);

    let report = scan
        .execute(ScanGapsRequest {
            video_a: path_a,
            video_b: path_b,
            align: aligned_chirp_config(),
            decode_chunk_secs: 10,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 10.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("scan should succeed");

    let gap = report
        .gaps
        .iter()
        .find(|g| {
            (g.video_a_start_secs - 40.0).abs() < 0.5 && (g.video_a_end_secs - 42.0).abs() < 0.5
        })
        .expect("should detect the 2-second gap near t=40s");

    assert!(
        gap.b_has_energy,
        "B should have audio at the corresponding position"
    );
    assert!(
        gap.duration_secs() >= 1.5,
        "gap should be at least 1.5s, got {}s",
        gap.duration_secs()
    );
}

/// A5 (`docs/dev/archive/TEMP-b-stall-warning-plan.md` § 4): a 20 s stretch of B zeroed while A keeps
/// program is reported as a B stall, and an A gap after it is attributed to that stall.
#[test]
fn execute_reports_b_stall_and_attributes_later_gap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_b, SAMPLE_RATE, 70, 90);
    zero_wav_segment(&path_a, SAMPLE_RATE, 104, 106);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let scan = ScanGaps::new(&media_reader, &progress, &aligner);

    let report = scan
        .execute(ScanGapsRequest {
            video_a: path_a,
            video_b: path_b,
            align: aligned_chirp_config(),
            decode_chunk_secs: 10,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 10.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("scan should succeed");

    let b_stall = report.b_stall.as_ref().expect("a real scan stamps b_stall");
    assert_eq!(b_stall.stall_min_secs, 10.0);
    assert_eq!(b_stall.stalls.len(), 1, "got {:?}", b_stall.stalls);
    let stall = &b_stall.stalls[0];
    assert!(
        (stall.b_start_secs - 70.0).abs() < 1.0 && (stall.duration_secs - 20.0).abs() < 1.0,
        "got {stall:?}"
    );
    assert!(stall.a_silent_fraction < 0.5, "got {stall:?}");
    // The chirp is constant-amplitude, so its near-flat block envelope correlates at every lag:
    // the shoulder test cannot tell step from flat and the stall is reported on length alone.
    assert_eq!(
        stall.evidence,
        Some(BStallEvidence::Length),
        "got {stall:?}"
    );
    assert!(stall.verdict_reason.is_some(), "got {stall:?}");
    assert_eq!(stall.shift_secs, stall.duration_secs, "got {stall:?}");

    let gap_index = report
        .gaps
        .iter()
        .position(|g| (g.video_a_start_secs - 104.0).abs() < 0.5)
        .expect("should detect the A gap near t=104s");
    let gap_starts: Vec<f64> = report.gaps.iter().map(|g| g.video_a_start_secs).collect();
    let attribution =
        clip_sync_repair::domain::b_stall::attribute_gaps_to_b_stalls(&gap_starts, &b_stall.stalls);
    assert_eq!(attribution[gap_index], Some(0));
}

/// Deterministic gain (dB, in [−24, 0)) for the `step`-th 500 ms step. Non-periodic, so the block
/// envelope registers at one lag only.
fn modulation_db(step: u64) -> f64 {
    let mut x = step.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    -24.0 * (x % 10_000) as f64 / 10_000.0
}

/// The corpus chirp sweep with a non-periodic amplitude envelope, so block levels carry shoulder
/// evidence (the plain chirp's envelope is flat). `delay_secs` of leading silence, like
/// `write_offset_chirp_wav_pair`'s B.
fn modulated_chirp(sample_rate: u32, total_secs: u32, delay_secs: u32) -> Vec<i16> {
    let rate = f64::from(sample_rate);
    let delay = u64::from(sample_rate) * u64::from(delay_secs);
    (0..u64::from(sample_rate) * u64::from(total_secs))
        .map(|index| {
            let Some(i) = index.checked_sub(delay) else {
                return 0;
            };
            let t = i as f64 / rate;
            let freq = 300.0 + 400.0 * t;
            let gain = 10f64.powf(modulation_db((t / 0.5) as u64) / 20.0);
            ((std::f64::consts::TAU * freq * t).sin() * gain * (i16::MAX as f64 * 0.5)).round()
                as i16
        })
        .collect()
}

fn write_mono_wav(path: &Path, sample_rate: u32, samples: &[i16]) {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for &sample in samples {
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// A B **stall**: `secs` of zeros spliced in at `at_secs`; later samples shift and the file grows.
fn insert_silence(samples: &mut Vec<i16>, sample_rate: u32, at_secs: u32, secs: u32) {
    let at = (sample_rate * at_secs) as usize;
    let zeros = std::iter::repeat_n(0, (sample_rate * secs) as usize);
    samples.splice(at..at, zeros);
}

#[test]
fn execute_confirms_short_b_stall_by_shoulders() {
    // B = A delayed 3 s, then two 3 s silences: a dropout at B 40 (program replaced, timeline
    // holds) and a stall at B 80 (program paused, timeline shifts). Both sit after the 60 s
    // start clip; the end clip sees the step and is not asserted on (A6's concern).
    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_mono_wav(
        &path_a,
        SAMPLE_RATE,
        &modulated_chirp(SAMPLE_RATE, TOTAL_SECS, 0),
    );
    let mut b = modulated_chirp(SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    let (drop_from, drop_to) = ((SAMPLE_RATE * 40) as usize, (SAMPLE_RATE * 43) as usize);
    b[drop_from..drop_to].fill(0);
    insert_silence(&mut b, SAMPLE_RATE, 80, 3);
    write_mono_wav(&path_b, SAMPLE_RATE, &b);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let scan = ScanGaps::new(&media_reader, &progress, &aligner);

    let report = scan
        .execute(ScanGapsRequest {
            video_a: path_a,
            video_b: path_b,
            align: aligned_chirp_config(),
            decode_chunk_secs: 10,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 2.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("scan should succeed");

    let b_stall = report.b_stall.as_ref().expect("a real scan stamps b_stall");
    assert_eq!(b_stall.stall_min_secs, 2.0);
    assert_eq!(
        b_stall.stalls.len(),
        1,
        "only the stall reports: {:?}",
        b_stall.stalls
    );
    let stall = &b_stall.stalls[0];
    assert!((stall.b_start_secs - 80.0).abs() < 0.5, "got {stall:?}");
    assert_eq!(
        stall.evidence,
        Some(BStallEvidence::Shoulder),
        "got {stall:?}"
    );
    assert!(
        (stall.shift_secs - 3.0).abs() <= 0.25 + 1e-9,
        "measured shift within one 250 ms block of 3 s: {stall:?}"
    );
}

/// Smoke test: B is a dual-track MP4 (2ch AAC + 6ch AC-3). The pipeline should select a
/// decodable track, align, detect the silent gap in A, and report B energy at that position.
#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
#[test]
fn ac3_dual_track_b_scan_detects_gap() {
    if !ffmpeg_util::ffmpeg_available() {
        eprintln!("skipping ac3_dual_track_b_scan_detects_gap: ffmpeg unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b_wav) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);

    let path_b_mp4 = temp.path().join("b_dual_ac3.mp4");
    if !ffmpeg_util::write_dual_track_aac_ac3_mp4(&path_b_wav, &path_b_mp4) {
        eprintln!("skipping: ffmpeg could not encode dual-track AAC+AC-3 MP4");
        return;
    }

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let scan = ScanGaps::new(&media_reader, &progress, &aligner);

    let report = scan
        .execute(ScanGapsRequest {
            video_a: path_a,
            video_b: path_b_mp4,
            align: aligned_chirp_config(),
            decode_chunk_secs: 10,
            recipe: clip_sync_repair::domain::ScanRecipe::with_hold_blocks(
                25_000, 0, 250, 0.01, 0.0,
            ),
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            stall_min_secs: 10.0,
            limit_fill_to_mapped_region: true,
            apply_donor_registration: true,
            local_pair_collar_secs: 2.0,
        })
        .expect("scan should succeed with dual-track AC-3 B");

    assert!(
        !report.gaps.is_empty(),
        "expected at least one silent gap in video A"
    );
    let silent_gap = report
        .gaps
        .iter()
        .find(|gap| {
            (gap.video_a_start_secs - f64::from(SILENT_START_SECS)).abs() < 1.0
                && (gap.video_a_end_secs - f64::from(SILENT_END_SECS)).abs() < 1.0
        })
        .expect("expected a gap covering the muted A segment");
    assert!(
        silent_gap.b_has_energy,
        "dual-track B (AAC + AC-3) should have audio at the aligned gap position"
    );
    assert!(silent_gap.is_fillable());
}
