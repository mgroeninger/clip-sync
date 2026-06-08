use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::{AlignmentResult, ClipLabel, ClipMatch, SymphoniaMediaReader};
use clip_sync_repair::application::{PatchAudio, PatchAudioRequest};
use clip_sync_repair::application::ports::PatchedAudioWriter;
use clip_sync_repair::domain::gap::{Gap, GapReport};
use clip_sync_repair::domain::{CompatibilityVerdict, TrackCompatibility};
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

fn patch_to_samples(request: PatchAudioRequest, crossfade_ms: u64) -> (Vec<i16>, u32, u16) {
    let progress = FakeProgressReporter;
    let media_reader = SymphoniaMediaReader;

    let patched = PatchAudio::new(&media_reader, &progress)
        .execute(request, crossfade_ms)
        .expect("patch should succeed");

    let temp = tempfile::tempdir().expect("tempdir");
    let path_out = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&patched, &path_out)
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

    let request = PatchAudioRequest {
        report: make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        normalize_fill: false,
        normalize_window_secs: 5.0,
        max_fill_gain_db: 12.0,
        // Both A's pre-gap border and B's fill are 440 Hz sine; correlation is ~1.0.
        min_fill_correlation: 0.35,
    };

    let (samples, sample_rate, channels) = patch_to_samples(request, 10);
    assert_eq!(sample_rate, SAMPLE_RATE);
    assert_eq!(channels, CHANNELS);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "gap region should have audio after fill, got rms={gap_rms}"
    );

    let pre_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(pre_rms > 100.0, "pre-gap region should have audio, got rms={pre_rms}");

    let post_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, 7.0, 9.0);
    assert!(
        post_rms > 100.0,
        "post-gap region should retain A audio after splice, got rms={post_rms}"
    );
}

#[test]
fn patch_audio_skips_fill_when_boundary_correlation_below_threshold() {
    let temp = tempfile::tempdir().expect("tempdir");
    // A borders are 440 Hz; B fill is 2200 Hz — uncorrelated at the seam.
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 2200.0, 16_000.0);

    let request = PatchAudioRequest {
        report: make_report(
            fixture.path_a,
            fixture.path_b,
            stereo_identical_compat(SAMPLE_RATE),
        ),
        normalize_fill: false,
        normalize_window_secs: 5.0,
        max_fill_gain_db: 12.0,
        min_fill_correlation: 0.35,
    };

    let (samples, _, _) = patch_to_samples(request, 10);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms < 50.0,
        "gap should stay silent when boundary correlation fails, got rms={gap_rms}"
    );

    // Borders outside the gap should still have A's original sine.
    let pre_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, 0.0, 2.0);
    assert!(pre_rms > 100.0, "pre-gap audio should be preserved, got rms={pre_rms}");
}

#[test]
fn patch_audio_normalizes_fill_loudness_to_a_border() {
    let temp = tempfile::tempdir().expect("tempdir");
    // B is the same frequency but much quieter than A's borders.
    let fixture = sine_gap_fixture(temp.path(), SAMPLE_RATE, SAMPLE_RATE, 440.0, 2_500.0);

    let (unnormalized, _, _) = patch_to_samples(
        PatchAudioRequest {
            report: make_report(
                fixture.path_a.clone(),
                fixture.path_b.clone(),
                stereo_identical_compat(SAMPLE_RATE),
            ),
            normalize_fill: false,
            normalize_window_secs: 2.0,
            max_fill_gain_db: 12.0,
            min_fill_correlation: 0.35,
        },
        10,
    );
    let (normalized, _, _) = patch_to_samples(
        PatchAudioRequest {
            report: make_report(
                fixture.path_a,
                fixture.path_b,
                stereo_identical_compat(SAMPLE_RATE),
            ),
            normalize_fill: true,
            normalize_window_secs: 2.0,
            max_fill_gain_db: 12.0,
            min_fill_correlation: 0.35,
        },
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

    let request = PatchAudioRequest {
        report: make_report(fixture.path_a, fixture.path_b, stereo_compatible_diff_rate()),
        normalize_fill: false,
        normalize_window_secs: 5.0,
        max_fill_gain_db: 12.0,
        // Resampler phase at the seam can depress correlation slightly; isolate resample behaviour.
        min_fill_correlation: -1.0,
    };

    let (samples, sample_rate, channels) = patch_to_samples(request, 10);
    assert_eq!(sample_rate, SAMPLE_RATE, "output should be at A's native rate");
    assert_eq!(channels, CHANNELS);

    let gap_rms = rms_region(&samples, SAMPLE_RATE, CHANNELS, GAP_START, GAP_END);
    assert!(
        gap_rms > 100.0,
        "gap should be filled after resampling B from {RATE_B} Hz, got rms={gap_rms}"
    );
}
