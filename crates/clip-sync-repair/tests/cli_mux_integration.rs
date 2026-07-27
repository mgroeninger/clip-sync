//! CLI `--mux` integration (`ffmpeg-mux` feature).
//!
//! Tier: **integration**. Mux argument parsing and ffmpeg subprocess adapter. `mux_writes_video` and
//! `mux_24bit_source_pipe_completes_successfully` are `#[ignore]` (need ffmpeg + disk I/O).
//!
//! PR: **compile + fast rows** when ffmpeg on PATH — `pr-repair`. E2e mux rows:
//! `.\scripts\test-tier.ps1 -Tier validation` (ffmpeg required).
//!
//! Run: `cargo test -p clip-sync-repair --features ffmpeg-mux --test cli_mux_integration`

use std::process::Command;

use clip_sync::testing::audio_fixtures::write_offset_chirp_wav_pair;

#[cfg(feature = "ffmpeg-mux")]
use std::path::Path;

#[cfg(feature = "ffmpeg-mux")]
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

#[cfg(feature = "ffmpeg-mux")]
use clip_sync::testing::fakes::FakeProgressReporter;
#[cfg(feature = "ffmpeg-mux")]
use clip_sync::{BitDepth, MultiChannelPcm};
#[cfg(feature = "ffmpeg-mux")]
use clip_sync_repair::application::ports::{MediaMuxer, MuxOptions};
#[cfg(feature = "ffmpeg-mux")]
use clip_sync_repair::infrastructure::ffmpeg_mux::FfmpegMediaMuxer;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECS: u32 = 120;
const OFFSET_SECS: u32 = 3;
#[cfg(feature = "ffmpeg-mux")]
const SILENT_START_SECS: u32 = 30;
#[cfg(feature = "ffmpeg-mux")]
const SILENT_END_SECS: u32 = 60;

#[cfg(feature = "ffmpeg-mux")]
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

#[cfg(feature = "ffmpeg-mux")]
fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(feature = "ffmpeg-mux")]
fn wav_to_mp4_with_black_video(wav: &Path, mp4: &Path) -> bool {
    Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s=320x240:d={TOTAL_SECS}"),
            "-i",
            wav.to_str().expect("wav utf8"),
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            "-movflags",
            "+faststart",
            mp4.to_str().expect("mp4 utf8"),
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
#[cfg(not(feature = "ffmpeg-mux"))]
fn mux_arg_rejected_without_feature() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    let out = temp.path().join("out.mp4");
    let bin = env!("CARGO_BIN_EXE_clip-sync-repair");

    let output = Command::new(bin)
        .args([
            path_a.to_str().expect("path_a"),
            path_b.to_str().expect("path_b"),
            "--mux",
            out.to_str().expect("out"),
        ])
        .output()
        .expect("run clip-sync-repair");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ffmpeg-mux"),
        "expected feature hint in stderr, got: {stderr}"
    );
}

#[cfg(feature = "ffmpeg-mux")]
#[test]
#[ignore = "tier:validation — needs ffmpeg + ffprobe on PATH; test-tier.ps1 -Tier validation"]
fn mux_writes_video() {
    if !ffmpeg_available() {
        eprintln!("skipping mux_writes_video: ffmpeg unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);

    let mp4_a = temp.path().join("a.mp4");
    assert!(
        wav_to_mp4_with_black_video(&path_a, &mp4_a),
        "failed to build test MP4 from WAV"
    );

    let out_mp4 = temp.path().join("repaired.mp4");
    let config_path = temp.path().join("repair.toml");
    std::fs::write(
        &config_path,
        r#"
[clip]
clip_length = 60

[repair]
min_fill_correlation = -1.0
scan_both = false
"#,
    )
    .expect("write config");

    let bin = env!("CARGO_BIN_EXE_clip-sync-repair");
    let status = Command::new(bin)
        .args([
            mp4_a.to_str().expect("mp4_a"),
            path_b.to_str().expect("path_b"),
            "--config",
            config_path.to_str().expect("config"),
            "--mux",
            out_mp4.to_str().expect("out_mp4"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ])
        .status()
        .expect("run clip-sync-repair");

    assert!(
        status.success(),
        "CLI should exit 0 on successful scan + mux"
    );
    assert!(out_mp4.exists(), "muxed MP4 should exist");
    assert!(
        out_mp4.metadata().expect("stat").len() > 1024,
        "muxed MP4 should be non-trivial size"
    );
}

/// Verifies that the mux pipe path (stdin PCM write) completes successfully when the
/// source `MultiChannelPcm` carries a 24-bit depth, i.e. that `write_pcm_le` is called
/// with `WavBitDepth::Int24` and ffmpeg accepts s24le input without error.
#[cfg(feature = "ffmpeg-mux")]
#[test]
#[ignore = "tier:validation — needs ffmpeg on PATH and disk I/O; test-tier.ps1 -Tier validation"]
fn mux_24bit_source_pipe_completes_successfully() {
    if !ffmpeg_available() {
        eprintln!("skipping mux_24bit_source_pipe_completes_successfully: ffmpeg unavailable");
        return;
    }

    const SECS: u32 = 10;
    const RATE: u32 = 48_000;
    const CHANNELS: u16 = 2;

    let temp = tempfile::tempdir().expect("tempdir");

    // Build a short silent WAV to wrap into an MP4 source
    let wav_path = temp.path().join("source.wav");
    {
        let spec = WavSpec {
            channels: CHANNELS,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&wav_path, spec).expect("write wav");
        for _ in 0..(SECS as usize * RATE as usize * CHANNELS as usize) {
            writer.write_sample(0i16).expect("write sample");
        }
        writer.finalize().expect("finalize");
    }

    let mp4_path = temp.path().join("source.mp4");
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s=320x240:d={SECS}"),
            "-i",
            wav_path.to_str().expect("wav utf8"),
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            "-movflags",
            "+faststart",
            mp4_path.to_str().expect("mp4 utf8"),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "failed to create source MP4 for mux test");

    // Construct PCM with 24-bit source depth — triggers s24le pipe path
    let pcm = MultiChannelPcm {
        sample_rate: RATE,
        channels: CHANNELS,
        samples: vec![0.0f32; SECS as usize * RATE as usize * CHANNELS as usize],
        decode_error_skips: 0,
        decoded_frame_count: None,
        compressed_bytes: None,
        source_bit_depth: Some(BitDepth::Int24),
    };

    let out_mp4 = temp.path().join("out.mp4");
    let options = MuxOptions {
        video_codec: "copy".into(),
        audio_codec: "aac".into(),
        audio_bitrate: None,
    };

    FfmpegMediaMuxer
        .mux_video_with_replaced_audio(&mp4_path, &pcm, &out_mp4, &options, &FakeProgressReporter)
        .expect("mux should succeed with 24-bit source");

    assert!(out_mp4.exists(), "output MP4 should exist");
    assert!(
        out_mp4.metadata().expect("stat").len() > 1024,
        "output MP4 should be non-trivial size"
    );
}
