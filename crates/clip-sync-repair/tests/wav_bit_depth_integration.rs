//! WAV output bit-depth integration.
//!
//! Tier: **integration**. `MultiChannelPcm` with varied `source_bit_depth` through
//! `WavPatchedAudioWriter` — end-to-end `resolve_output_bit_depth` wiring without CLI.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test wav_bit_depth_integration`

use clip_sync::{BitDepth, MultiChannelPcm};
use clip_sync_repair::application::ports::PatchedAudioWriter;
use clip_sync_repair::infrastructure::wav_writer::WavPatchedAudioWriter;
use hound::{SampleFormat, WavReader};

fn make_pcm(source_bit_depth: Option<BitDepth>) -> MultiChannelPcm {
    MultiChannelPcm {
        sample_rate: 48_000,
        channels: 2,
        samples: vec![0.0f32; 48_000 * 2],
        decode_error_skips: 0,
        decoded_frame_count: None,
        compressed_bytes: None,
        source_bit_depth,
    }
}

// --- depth resolution ---

#[test]
fn wav_writer_24bit_int_source_produces_24bit_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&make_pcm(Some(BitDepth::Int24)), &path)
        .expect("write");
    let spec = WavReader::open(&path).expect("open").spec();
    assert_eq!(spec.bits_per_sample, 24);
    assert_eq!(spec.sample_format, SampleFormat::Int);
}

#[test]
fn wav_writer_float32_source_produces_24bit_int_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&make_pcm(Some(BitDepth::Float32)), &path)
        .expect("write");
    let spec = WavReader::open(&path).expect("open").spec();
    assert_eq!(spec.bits_per_sample, 24);
    assert_eq!(spec.sample_format, SampleFormat::Int);
}

#[test]
fn wav_writer_int32_source_produces_24bit_int_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&make_pcm(Some(BitDepth::Int32)), &path)
        .expect("write");
    let spec = WavReader::open(&path).expect("open").spec();
    assert_eq!(spec.bits_per_sample, 24);
    assert_eq!(spec.sample_format, SampleFormat::Int);
}

#[test]
fn wav_writer_lossy_source_stays_16bit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&make_pcm(None), &path)
        .expect("write");
    let spec = WavReader::open(&path).expect("open").spec();
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, SampleFormat::Int);
}

#[test]
fn wav_writer_16bit_source_stays_16bit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&make_pcm(Some(BitDepth::Int16)), &path)
        .expect("write");
    let spec = WavReader::open(&path).expect("open").spec();
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, SampleFormat::Int);
}

// --- 24-bit sample value round-trip ---

#[test]
fn wav_writer_24bit_sample_values_round_trip() {
    let mut pcm = make_pcm(Some(BitDepth::Int24));
    pcm.samples[0] = 0.5;
    pcm.samples[1] = -0.25;

    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("out.wav");
    WavPatchedAudioWriter
        .write(&pcm, &path)
        .expect("write");

    let mut reader = WavReader::open(&path).expect("open");
    let out: Vec<i32> = reader
        .samples::<i32>()
        .take(2)
        .map(|s| s.expect("read"))
        .collect();

    // f32_to_i24 scales by 8_388_607.0 and rounds
    let expected_0 = (0.5_f32 * 8_388_607.0).round() as i32;
    let expected_1 = (-0.25_f32 * 8_388_607.0).round() as i32;
    assert_eq!(out[0], expected_0, "sample 0");
    assert_eq!(out[1], expected_1, "sample 1");
}
