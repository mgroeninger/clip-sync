use std::path::Path;
use std::time::Duration;

use clip_sync::ClipConfig;
use clip_sync::testing::audio_fixtures::write_offset_chirp_wav_pair;
use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{AlignConfig, SymphoniaMediaReader};
use clip_sync_repair::application::{ScanGaps, ScanGapsRequest};
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
    let (path_a, path_b) = write_offset_chirp_wav_pair(
        temp.path(),
        SAMPLE_RATE,
        TOTAL_SECS,
        OFFSET_SECS,
    );
    zero_wav_segment(
        &path_a,
        SAMPLE_RATE,
        SILENT_START_SECS,
        SILENT_END_SECS,
    );

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
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            silence_hold_blocks: 0,
            min_gap_secs: 25.0,
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
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
    let (path_a, path_b) = write_offset_chirp_wav_pair(
        temp.path(),
        SAMPLE_RATE,
        TOTAL_SECS,
        OFFSET_SECS,
    );
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
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            silence_hold_blocks: 0,
            min_gap_secs: 1.0,
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
        })
        .expect("scan should succeed");

    let gap = report
        .gaps
        .iter()
        .find(|g| (g.video_a_start_secs - 40.0).abs() < 0.5 && (g.video_a_end_secs - 42.0).abs() < 0.5)
        .expect("should detect the 2-second gap near t=40s");

    assert!(gap.b_has_energy, "B should have audio at the corresponding position");
    assert!(gap.duration_secs() >= 1.5, "gap should be at least 1.5s, got {}s", gap.duration_secs());
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
    let (path_a, path_b_wav) = write_offset_chirp_wav_pair(
        temp.path(),
        SAMPLE_RATE,
        TOTAL_SECS,
        OFFSET_SECS,
    );
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
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            silence_hold_blocks: 0,
            min_gap_secs: 25.0,
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
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
