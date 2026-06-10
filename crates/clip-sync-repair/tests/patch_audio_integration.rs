use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::MultiChannelPcm;
use clip_sync::{
    AlignmentResult, ClipLabel, ClipMatch, MediaReader, MediaSession, SymphoniaMediaReader,
};
use clip_sync_repair::application::{PatchAudio, PatchAudioRequest};
use clip_sync_repair::domain::policies;
use clip_sync_repair::domain::GapPatchStatus;
use clip_sync_repair::application::ports::PatchedAudioWriter;
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::{CompatibilityVerdict, TrackCompatibility};
use clip_sync_repair::infrastructure::aligner::SymphoniaAligner;
use clip_sync_repair::infrastructure::wav_writer::WavPatchedAudioWriter;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

const SAMPLE_RATE: u32 = 44_100;
const RATE_B: u32 = 48_000;
const CHANNELS: u16 = 2;

const GAP_START: f64 = 3.0;
const GAP_END: f64 = 6.0;
const TOTAL_SECS: u32 = 10;

fn sine_sample(sample_rate: u32, index: u64, freq: f32, amplitude: f32) -> i16 {
    let t = index as f32 / sample_rate as f32;
    (f32::sin(TAU * freq * t) * amplitude).round() as i16
}

/// Write a stereo WAV with a sine wave for `total_secs`.
fn write_stereo_sine_wav(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    for frame in 0..total_frames {
        let s = sine_sample(sample_rate, frame, freq, amplitude);
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

/// Write a stereo WAV with a sine wave and a silent gap.
fn write_stereo_sine_with_gap(
    path: &Path,
    sample_rate: u32,
    total_secs: u32,
    gap_start_secs: u32,
    gap_end_secs: u32,
    freq: f32,
    amplitude: f32,
) {
    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    let total_frames = u64::from(sample_rate) * u64::from(total_secs);
    let gap_start_frame = u64::from(sample_rate) * u64::from(gap_start_secs);
    let gap_end_frame = u64::from(sample_rate) * u64::from(gap_end_secs);

    for frame in 0..total_frames {
        let s = if frame >= gap_start_frame && frame < gap_end_frame {
            0i16
        } else {
            sine_sample(sample_rate, frame, freq, amplitude)
        };
        writer.write_sample(s).expect("write L");
        writer.write_sample(s).expect("write R");
    }
    writer.finalize().expect("finalize wav");
}

fn rms_region(samples: &[i16], sample_rate: u32, channels: u16, start_secs: f64, end_secs: f64) -> f32 {
    let start = (start_secs * sample_rate as f64) as usize * channels as usize;
    let end = (end_secs * sample_rate as f64) as usize * channels as usize;
    let end = end.min(samples.len());
    if start >= end {
        return 0.0;
    }
    let slice = &samples[start..end];
    let sum_sq: f64 = slice.iter().map(|&s| { let v = f64::from(s); v * v }).sum();
    (sum_sq / slice.len() as f64).sqrt() as f32
}

fn make_alignment(offset: f64) -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 10.0,
            aligned: true,
            offset_secs: Some(offset),
            confidence: 0.9,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(offset),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
    }
}

fn stereo_identical_compat(sample_rate: u32) -> TrackCompatibility {
    TrackCompatibility {
        a_channels: CHANNELS,
        b_channels: CHANNELS,
        a_sample_rate: sample_rate,
        b_sample_rate: sample_rate,
        channels_match: true,
        rate_match: true,
        verdict: CompatibilityVerdict::Identical,
    }
}

fn stereo_compatible_diff_rate() -> TrackCompatibility {
    TrackCompatibility {
        a_channels: CHANNELS,
        b_channels: CHANNELS,
        a_sample_rate: SAMPLE_RATE,
        b_sample_rate: RATE_B,
        channels_match: true,
        rate_match: false,
        verdict: CompatibilityVerdict::Compatible,
    }
}

fn default_gap() -> Gap {
    Gap {
        video_a_start_secs: GAP_START,
        video_a_end_secs: GAP_END,
        video_b_start_secs: Some(GAP_START),
        video_b_end_secs: Some(GAP_END),
        b_has_energy: true,
    }
}

fn patch_request(
    report: GapReport,
    normalize_fill: bool,
    normalize_window_secs: f64,
    min_fill_correlation: f32,
) -> PatchAudioRequest {
    PatchAudioRequest {
        report,
        normalize_fill,
        normalize_window_secs,
        max_fill_gain_db: 12.0,
        min_fill_correlation,
        fill_align_margin_secs: 1.0,
        max_fill_align_adjustment_secs: 1.0,
        fill_border_search_secs: 30.0,
        min_border_discovery_secs: 2.0,
        border_standoff_secs: 0.0,
        short_gap_mean_correlation_secs: 2.0,
        fill_length_slack_secs: 5.0,
        fill_seam_search_secs: 0.25,
        gap_signature_context_secs: 3.0,
        gap_signature_bin_ms: 50,
        min_structure_match_score: 0.55,
        strong_structure_trust: 0.90,
        partial_structure_waveform_soften: 0.85,
        absolute_silence_rms: 0.0,
    }
}

fn make_report(path_a: PathBuf, path_b: PathBuf, compat: TrackCompatibility) -> GapReport {
    GapReport {
        video_a: path_a,
        video_b: path_b,
        track_compatibility: Some(compat),
        overlap: None,
        alignment: make_alignment(0.0),
        gaps: vec![default_gap()],
        gap_offset_agreement: None,
        decode_chunk_secs: 60,
        scan_block_ms: 250,
        silence_peak_fraction: 0.01,
    }
}

fn expect_pcm(result: &clip_sync_repair::application::PatchAudioResult) -> &MultiChannelPcm {
    result
        .pcm
        .as_ref()
        .expect("expected patch to decode A when fill regions exist")
}

fn patch_to_samples(request: PatchAudioRequest, crossfade_ms: u64) -> (Vec<i16>, u32, u16) {
    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;

    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, crossfade_ms)
        .expect("patch should succeed");

    let temp = tempfile::tempdir().expect("tempdir");
    let path_out = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(expect_pcm(&result), &path_out)
        .expect("write should succeed");

    let mut reader = WavReader::open(&path_out).expect("open out.wav");
    let spec = reader.spec();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    (samples, spec.sample_rate, spec.channels)
}

struct SineGapFixture {
    path_a: PathBuf,
    path_b: PathBuf,
}

fn sine_gap_fixture(
    temp: &Path,
    rate_a: u32,
    rate_b: u32,
    b_freq: f32,
    b_amplitude: f32,
) -> SineGapFixture {
    let path_a = temp.join("a.wav");
    let path_b = temp.join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        rate_a,
        TOTAL_SECS,
        GAP_START as u32,
        GAP_END as u32,
        440.0,
        16_000.0,
    );
    write_stereo_sine_wav(&path_b, rate_b, TOTAL_SECS, b_freq, b_amplitude);
    SineGapFixture { path_a, path_b }
}

#[test]
fn patch_audio_fills_gap_in_stereo_wav() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let request = patch_request(
        make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        false,
        5.0,
        // Both A's pre-gap border and B's fill are 440 Hz sine; correlation is ~1.0.
        0.35,
    );

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");
    let pcm = expect_pcm(&result);
    assert_eq!(pcm.sample_rate, SAMPLE_RATE);
    assert_eq!(pcm.channels, CHANNELS);
    assert_eq!(result.summary.patched_count, 1);

    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "gap region should have audio after fill, got rms={gap_rms}"
    );

    let pre_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(pre_rms > 100.0, "pre-gap region should have audio, got rms={pre_rms}");

    let post_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 7.0, 9.0);
    assert!(
        post_rms > 100.0,
        "post-gap region should retain A audio after splice, got rms={post_rms}"
    );
}

#[test]
fn patch_audio_trusts_structure_when_waveform_seams_disagree() {
    let temp = tempfile::tempdir().expect("tempdir");
    // A borders are 440 Hz; B fill is 2200 Hz — waveform Pearson fails, but the
    // active/silent edit pattern still matches (structure-first placement).
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 2200.0, 16_000.0);

    let request = patch_request(
        make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        false,
        5.0,
        0.35,
    );

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "gap should be filled when structure match is strong, got rms={gap_rms}"
    );

    let pre_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(pre_rms > 100.0, "pre-gap audio should be preserved, got rms={pre_rms}");

    assert_eq!(result.summary.patched_count, 1);
    assert_eq!(result.summary.skipped_count, 0);
    match &result.summary.gaps[0].status {
        GapPatchStatus::Patched {
            structure_trusted: true,
            ..
        } => {}
        other => panic!("expected structure-trusted patch, got {other:?}"),
    }
}

#[test]
fn patch_audio_normalizes_fill_loudness_to_a_border() {
    let temp = tempfile::tempdir().expect("tempdir");
    // B is the same frequency but much quieter than A's borders.
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 2_500.0);

    let (unnormalized, _, _) = patch_to_samples(
        patch_request(
            make_report(
                fixture.path_a.clone(),
                fixture.path_b.clone(),
                stereo_identical_compat(SAMPLE_RATE),
            ),
            false,
            2.0,
            0.35,
        ),
        10,
    );
    let (normalized, _, _) = patch_to_samples(
        patch_request(
            make_report(
                fixture.path_a,
                fixture.path_b,
                stereo_identical_compat(SAMPLE_RATE),
            ),
            true,
            2.0,
            0.35,
        ),
        10,
    );

    let pre_rms = rms_region(&unnormalized, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    // Measure the gap interior (away from crossfade seams).
    const GAP_INNER_START: f64 = 3.5;
    const GAP_INNER_END: f64 = 5.5;

    let gap_unnorm =
        rms_region(&unnormalized, SAMPLE_RATE, CHANNELS, GAP_INNER_START, GAP_INNER_END);
    let gap_norm = rms_region(&normalized, SAMPLE_RATE, CHANNELS, GAP_INNER_START, GAP_INNER_END);

    assert!(
        gap_unnorm < pre_rms * 0.4,
        "unnormalized fill should be much quieter than A border (pre={pre_rms}, gap={gap_unnorm})"
    );
    assert!(
        gap_norm > gap_unnorm * 2.0,
        "normalization should boost quiet B fill (unnorm={gap_unnorm}, norm={gap_norm})"
    );
    assert!(
        gap_norm > pre_rms * 0.55,
        "normalized gap should be close to A border level (pre={pre_rms}, gap={gap_norm})"
    );
}

#[test]
fn patch_audio_resamples_b_when_sample_rates_differ() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, RATE_B, 440.0, 16_000.0);

    let request = patch_request(
        make_report(fixture.path_a, fixture.path_b, stereo_compatible_diff_rate()),
        false,
        5.0,
        // Resampler phase at the seam can depress correlation slightly; isolate resample behaviour.
        -1.0,
    );

    let (samples, sample_rate, channels) = patch_to_samples(request, 10);
    assert_eq!(sample_rate, SAMPLE_RATE, "output should be at A's native rate");
    assert_eq!(channels, CHANNELS);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "gap should be filled after resampling B from {RATE_B} Hz, got rms={gap_rms}"
    );
}

#[test]
fn patch_audio_aligns_shifted_b_fill_to_a_borders() {
    const SHIFT_SECS: f64 = 0.05;

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let mut gap = default_gap();
    // Simulate a coarse alignment error: mapped B window is 50 ms late.
    gap.video_b_start_secs = Some(GAP_START + SHIFT_SECS);
    gap.video_b_end_secs = Some(GAP_END + SHIFT_SECS);

    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.gaps = vec![gap];

    let request = patch_request(report, false, 2.0, 0.35);

    let (samples, _, _) = patch_to_samples(request, 10);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "aligned fill should recover shifted B mapping, got rms={gap_rms}"
    );

    // Seam should stay continuous across the gap (no audible dip at boundaries).
    let pre_last = rms_region(&samples, SAMPLE_RATE, CHANNELS, 2.9, 3.0);
    let gap_open = rms_region(&samples, SAMPLE_RATE, CHANNELS, 3.0, 3.1);
    let gap_close = rms_region(&samples, SAMPLE_RATE, CHANNELS, 5.9, 6.0);
    let post_first = rms_region(&samples, SAMPLE_RATE, CHANNELS, 6.0, 6.1);

    assert!(pre_last > 100.0, "pre-gap should stay loud, got rms={pre_last}");
    assert!(
        gap_open > pre_last * 0.5,
        "gap opening should not dip to silence (pre={pre_last}, open={gap_open})"
    );
    assert!(
        gap_close > pre_last * 0.5,
        "gap closing should not dip to silence (pre={pre_last}, close={gap_close})"
    );
    assert!(post_first > 100.0, "post-gap should stay loud, got rms={post_first}");
}

#[test]
fn decoded_gap_frames_are_silent_in_sine_fixture() {
    use std::time::Duration;

    use clip_sync::{ClipLabel, ClipWindow, SymphoniaMediaReader};

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let media_reader = SymphoniaMediaReader;
    let session = media_reader
        .open(&clip_sync::MediaSource::new(fixture.path_a.clone()))
        .expect("open A");
    let tracks = session.list_tracks().expect("tracks");
    let track = &tracks[0];
    let duration = track.duration.expect("duration");
    let window = ClipWindow::new(Duration::ZERO, duration, ClipLabel::Interior);
    let pcm = session
        .extract_interleaved(track, &window, &FakeProgressReporter, "probe-a")
        .expect("extract A");

    let gap_frame = (GAP_START * SAMPLE_RATE as f64) as usize;
    let pre_frame = gap_frame.saturating_sub(1);
    assert!(
        !policies::is_silent_frame(&pcm.samples, CHANNELS as usize, pre_frame, 0.01, 0.0),
        "frame before gap should not be silent"
    );
    assert!(
        policies::is_silent_frame(&pcm.samples, CHANNELS as usize, gap_frame, 0.01, 0.0),
        "first gap frame should decode as silent"
    );
}

/// Scanner block quantization can report a gap that starts slightly early and ends slightly
/// early. Patch should refine edges and still splice B with passing seam correlation.
#[test]
fn patch_audio_fills_gap_with_imprecise_scan_boundaries() {
    const BOUNDARY_SLACK_SECS: f64 = 0.25;

    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 16_000.0);

    let imprecise_gap = Gap {
        video_a_start_secs: GAP_START - BOUNDARY_SLACK_SECS,
        video_a_end_secs: GAP_END - BOUNDARY_SLACK_SECS,
        video_b_start_secs: Some(GAP_START - BOUNDARY_SLACK_SECS),
        video_b_end_secs: Some(GAP_END - BOUNDARY_SLACK_SECS),
        b_has_energy: true,
    };

    let mut report = make_report(
        fixture.path_a,
        fixture.path_b,
        stereo_identical_compat(SAMPLE_RATE),
    );
    report.gaps = vec![imprecise_gap];

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let request = patch_request(report, false, 5.0, 0.35);
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(request, 10)
        .expect("patch should succeed");

    assert_eq!(
        result.summary.patched_count, 1,
        "expected patch to succeed, got {:?}",
        result.summary.gaps
    );

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(&pcm.samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "imprecise scan boundaries should still be filled, got rms={gap_rms}, status={:?}",
        result.summary.gaps[0].status
    );
}

#[test]
fn scan_then_patch_fills_detected_gap() {
    use std::time::Duration;

    use clip_sync::ClipConfig;
    use clip_sync::{AlignConfig, SymphoniaMediaReader};
    use clip_sync_repair::application::{ScanGaps, ScanGapsRequest};

    const SCAN_TOTAL_SECS: u32 = 120;
    const SCAN_GAP_START: u32 = 40;
    const SCAN_GAP_END: u32 = 43;

    let temp = tempfile::tempdir().expect("tempdir");
    let path_a = temp.path().join("a.wav");
    let path_b = temp.path().join("b.wav");
    write_stereo_sine_with_gap(
        &path_a,
        SAMPLE_RATE,
        SCAN_TOTAL_SECS,
        SCAN_GAP_START,
        SCAN_GAP_END,
        440.0,
        16_000.0,
    );
    write_stereo_sine_wav(&path_b, SAMPLE_RATE, SCAN_TOTAL_SECS, 440.0, 16_000.0);

    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let report = ScanGaps::new(&media_reader, &progress, &aligner)
        .execute(ScanGapsRequest {
            video_a: path_a.clone(),
            video_b: path_b.clone(),
            align: AlignConfig {
                clip: ClipConfig {
                    clip_length: Duration::from_secs(60),
                    num_clips: 1,
                    target_sample_rate: Some(SAMPLE_RATE),
                    ..ClipConfig::default()
                },
                ..Default::default()
            },
            decode_chunk_secs: 2,
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            silence_hold_blocks: 0,
            min_gap_secs: 1.0,
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
        })
        .expect("scan should succeed");

    assert!(
        report.fillable_count() >= 1,
        "scan should detect a fillable gap"
    );

    let patch_request = patch_request(report, false, 5.0, 0.35);
    let result = PatchAudio::new(&media_reader, &progress)
        .execute(patch_request, 10)
        .expect("patch should succeed");

    assert!(
        result.summary.patched_count >= 1,
        "expected at least one patched gap, got {:?}",
        result.summary.gaps
    );

    let pcm = expect_pcm(&result);
    let gap_rms = rms_region(
        &pcm.samples,
        SAMPLE_RATE,
        CHANNELS,
        f64::from(SCAN_GAP_START),
        f64::from(SCAN_GAP_END),
    );
    assert!(
        gap_rms > 100.0,
        "scan-detected gap should be filled, got rms={gap_rms}"
    );
}
