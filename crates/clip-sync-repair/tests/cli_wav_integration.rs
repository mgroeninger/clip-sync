//! CLI `--wav` integration (scan-gaps + patch-audio subprocess).
//!
//! Tier: **integration**. End-to-end CLI on mono chirp WAVs with a mid-file silence gap.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test cli_wav_integration`

use clip_sync::testing::audio_fixtures::write_offset_chirp_wav_pair;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::path::Path;
use std::process::Command;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECS: u32 = 120;
const OFFSET_SECS: u32 = 3;
// 30 s silence in the middle of A; detected via block-level scan (no bucket alignment needed).
const SILENT_START_SECS: u32 = 30;
const SILENT_END_SECS: u32 = 60;

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

fn rms_region_mono(samples: &[i16], sample_rate: u32, start_secs: f64, end_secs: f64) -> f32 {
    let start = (start_secs * sample_rate as f64) as usize;
    let end = (end_secs * sample_rate as f64) as usize;
    let end = end.min(samples.len());
    if start >= end {
        return 0.0;
    }
    let slice = &samples[start..end];
    let sum_sq: f64 = slice
        .iter()
        .map(|&s| {
            let v = f64::from(s);
            v * v
        })
        .sum();
    (sum_sq / slice.len() as f64).sqrt() as f32
}

/// End-to-end: align → scan → patch → WAV via the CLI `--wav` flag.
#[test]
fn cli_scan_and_wav_writes_patched_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);

    let out_wav = temp.path().join("patched.wav");
    let config_path = temp.path().join("repair.toml");
    // Chirp segments at the gap seam may not correlate; disable gate for this smoke test.
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
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--wav",
            out_wav.to_str().expect("out utf8"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ])
        .status()
        .expect("run clip-sync-repair");

    assert!(
        status.success(),
        "CLI should exit 0 on successful scan + WAV write"
    );
    assert!(out_wav.exists(), "patched WAV should exist");

    let mut reader = WavReader::open(&out_wav).expect("open patched wav");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();

    let gap_rms = rms_region_mono(
        &samples,
        SAMPLE_RATE,
        f64::from(SILENT_START_SECS),
        f64::from(SILENT_END_SECS),
    );
    assert!(
        gap_rms > 100.0,
        "patched WAV gap region should have audio after fill, got rms={gap_rms}"
    );
}

/// `--patch-only` runs the same patch as the `--wav` case above and reports it, writing nothing.
///
/// The measurement that motivates the flag is `fill_level`, which is taken during the splice and so
/// cannot come from `--repair-preview`. Asserting it is present *and* that no file appeared is the
/// whole contract: the corpus sweep needs the number, not the audio, and a throwaway `--wav` is what
/// puts long surround runs into the 4 GiB classic-WAV failure that discards the report.
#[test]
fn cli_patch_only_reports_fill_level_and_writes_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);

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

    // Everything the fixture put there; anything new after the run is a file patch-only wrote.
    let before = dir_entries(temp.path());

    let bin = env!("CARGO_BIN_EXE_clip-sync-repair");
    let output = Command::new(bin)
        .args([
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--patch-only",
            "--format",
            "json",
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ])
        .output()
        .expect("run clip-sync-repair");

    assert!(
        output.status.success(),
        "patch-only should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("patch-only emits a JSON report");
    let gaps = doc["patch"]["gaps"]
        .as_array()
        .expect("a patch block with gaps — patch-only must reach the splice, not stop at preview");
    let levels: Vec<&serde_json::Value> = gaps
        .iter()
        .filter_map(|gap| gap.get("fill_level"))
        .collect();
    assert!(
        !levels.is_empty(),
        "patch-only exists to produce fill_level; got none in {gaps:#?}"
    );
    assert!(
        levels[0]["peak_delta_db"].is_number(),
        "fill_level must carry the calibration statistic: {:#?}",
        levels[0]
    );

    let mut after = dir_entries(temp.path());
    after.retain(|name| !before.contains(name));
    assert!(
        after.is_empty(),
        "patch-only must leave no new files behind; found {after:?}"
    );
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("dir entry").file_name().to_string_lossy().into())
        .collect();
    names.sort();
    names
}
