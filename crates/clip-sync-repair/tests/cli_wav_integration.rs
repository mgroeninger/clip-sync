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
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into()
        })
        .collect();
    names.sort();
    names
}

/// `--seam-extend-fill` (the default) against `--no-seam-extend-fill`: the flag must reach the written
/// audio, and change **only** the fill.
///
/// The shipped seam path has `gap_frames` of fill for a region spanning `gap_frames + 2*cf`, and
/// covers the shortfall by replaying the fill tail — the blended block is written both inside the
/// gap and after it. The flag supplies the missing frames from B instead. Two consequences are
/// asserted here because they are what the ear pass is listening for: the gap interior changes
/// (its content is no longer displaced by one crossfade), and audio well outside the gap does not.
#[test]
fn seam_extend_fill_changes_the_fill_and_nothing_outside_it() {
    fn run(dir: &Path, extend: bool) -> Vec<i16> {
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(dir, SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
        zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);
        let out_wav = dir.join("patched.wav");
        let config_path = dir.join("repair.toml");
        std::fs::write(
            &config_path,
            "[clip]
clip_length = 60

[repair]
min_fill_correlation = -1.0
scan_both = false
",
        )
        .expect("write config");

        let bin = env!("CARGO_BIN_EXE_clip-sync-repair");
        let mut cmd = Command::new(bin);
        cmd.args([
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--wav",
            out_wav.to_str().expect("out utf8"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ]);
        cmd.arg(if extend {
            "--seam-extend-fill"
        } else {
            "--no-seam-extend-fill"
        });
        assert!(cmd.status().expect("run clip-sync-repair").success());
        let mut reader = WavReader::open(&out_wav).expect("open patched wav");
        reader
            .samples::<i16>()
            .map(|s| s.expect("read sample"))
            .collect()
    }

    let plain_dir = tempfile::tempdir().expect("tempdir");
    let ext_dir = tempfile::tempdir().expect("tempdir");
    let plain = run(plain_dir.path(), false);
    let extended = run(ext_dir.path(), true);

    assert_eq!(plain.len(), extended.len(), "splice is length-preserving");
    assert_ne!(
        plain, extended,
        "--seam-extend-fill must reach the written audio"
    );

    // One second clear of either gap edge: the flag changes the fill, not the programme around it.
    let before = (SAMPLE_RATE * (SILENT_START_SECS - 1)) as usize;
    let after = (SAMPLE_RATE * (SILENT_END_SECS + 1)) as usize;
    assert_eq!(
        plain[..before],
        extended[..before],
        "audio before the gap must be untouched"
    );
    assert_eq!(
        plain[after..],
        extended[after..],
        "audio after the gap must be untouched"
    );
}

/// Replace the `band_ms + ramp_ms` before `edge_secs` with a soft dropout edge: a 1 kHz tone at
/// −30 dBFS, then a gradual decay (3.25 dB/ms, never a 15 dB step inside two 2 ms bins) toward
/// silence. The gap's silence floor is reached inside the decay, so refinement stops there and the
/// band stays raw A unless the edge snap moves the write to the band's first frame.
fn soften_lead_edge(path: &Path, sample_rate: u32, edge_secs: u32, band_ms: f64, ramp_ms: f64) {
    let mut reader = WavReader::open(path).expect("open wav");
    let mut samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|sample| sample.expect("read sample"))
        .collect();
    let rate = f64::from(sample_rate);
    let edge = (u64::from(sample_rate) * u64::from(edge_secs)) as usize;
    let band = (band_ms / 1000.0 * rate).round() as usize;
    let ramp = (ramp_ms / 1000.0 * rate).round() as usize;
    let start = edge - band - ramp;
    for (k, sample) in samples[start..edge].iter_mut().enumerate() {
        let db = if k < band {
            -30.0
        } else {
            -30.0 - 3.25 * ((k - band) as f64 * 1000.0 / rate)
        };
        let phase = 2.0 * std::f64::consts::PI * 1000.0 * k as f64 / rate;
        *sample = (10f64.powf(db / 20.0) * phase.sin() * 32767.0).round() as i16;
    }
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for sample in samples {
        writer.write_sample(sample).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// `--edge-snap-max-ms` must move the write onto a soft lead band, and nothing well before it.
///
/// Without the snap, the band between A's dropout onset and the silence floor is raw A beside the
/// fill (`docs/dev/TEMP-soft-edge-census-plan.md` § 1). With it, the fill starts at the onset.
#[test]
fn edge_snap_writes_over_a_soft_lead_band() {
    const BAND_MS: f64 = 25.0;
    const RAMP_MS: f64 = 20.0;
    fn run(dir: &Path, snap: bool) -> (Vec<i16>, Vec<i16>) {
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(dir, SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
        zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);
        soften_lead_edge(&path_a, SAMPLE_RATE, SILENT_START_SECS, BAND_MS, RAMP_MS);
        let raw_a: Vec<i16> = WavReader::open(&path_a)
            .expect("open a")
            .samples::<i16>()
            .map(|s| s.expect("read sample"))
            .collect();
        let out_wav = dir.join("patched.wav");
        let config_path = dir.join("repair.toml");
        std::fs::write(
            &config_path,
            "[clip]
clip_length = 60

[repair]
min_fill_correlation = -1.0
scan_both = false
",
        )
        .expect("write config");
        let bin = env!("CARGO_BIN_EXE_clip-sync-repair");
        let mut cmd = Command::new(bin);
        cmd.args([
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--wav",
            out_wav.to_str().expect("out utf8"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ]);
        if snap {
            cmd.args(["--edge-snap-max-ms", "40"]);
        }
        assert!(cmd.status().expect("run clip-sync-repair").success());
        let patched = WavReader::open(&out_wav)
            .expect("open patched wav")
            .samples::<i16>()
            .map(|s| s.expect("read sample"))
            .collect();
        (raw_a, patched)
    }

    let off_dir = tempfile::tempdir().expect("tempdir");
    let on_dir = tempfile::tempdir().expect("tempdir");
    let (raw_a, off) = run(off_dir.path(), false);
    let (_, on) = run(on_dir.path(), true);
    assert_eq!(off.len(), on.len(), "splice is length-preserving");

    let rate = f64::from(SAMPLE_RATE);
    let edge = (SAMPLE_RATE * SILENT_START_SECS) as usize;
    let onset = edge - ((BAND_MS + RAMP_MS) / 1000.0 * rate).round() as usize;
    // The band's first 5 ms: clear of the unsnapped pre-seam crossfade, which opens near the floor.
    let probe = onset..onset + (0.005 * rate) as usize;
    assert_eq!(
        off[probe.clone()],
        raw_a[probe.clone()],
        "without the snap the band stays raw A"
    );
    assert_ne!(
        on[probe.clone()],
        raw_a[probe],
        "with the snap the fill covers the band"
    );
    let before = onset - (0.1 * rate) as usize;
    assert_eq!(
        off[..before],
        on[..before],
        "audio 100 ms before the onset must be untouched"
    );
}

/// `placement` must locate the write in the output and its fill in B.
///
/// Every sample the splice changed lies inside `[write_start_secs, write_end_secs)`, and the first
/// change opens the range (the crossfade's first frame weighs the fill at zero, so allow one
/// crossfade of slack). B is A delayed by `OFFSET_SECS`, so the fill frame at the gap's first frame
/// sits `OFFSET_SECS` later in B. The soft lead is moved by refinement, which `refine_start_frames`
/// must carry.
#[test]
fn placement_locates_the_write_and_the_fill() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);
    soften_lead_edge(&path_a, SAMPLE_RATE, SILENT_START_SECS, 25.0, 20.0);
    let raw_a: Vec<i16> = WavReader::open(&path_a)
        .expect("open a")
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    let out_wav = temp.path().join("patched.wav");
    let config_path = temp.path().join("repair.toml");
    std::fs::write(
        &config_path,
        "[clip]
clip_length = 60

[repair]
min_fill_correlation = -1.0
scan_both = false
",
    )
    .expect("write config");
    let bin = env!("CARGO_BIN_EXE_clip-sync-repair");
    let output = Command::new(bin)
        .args([
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--wav",
            out_wav.to_str().expect("out utf8"),
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
        "repair should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--format json emits a report");
    let gaps = doc["patch"]["gaps"].as_array().expect("a patch block");
    assert_eq!(gaps.len(), 1, "one gap: {gaps:#?}");
    let placement = &gaps[0]["placement"];
    let secs = |key: &str| {
        placement[key]
            .as_f64()
            .unwrap_or_else(|| panic!("placement.{key} missing: {placement:#?}"))
    };
    let rate = f64::from(SAMPLE_RATE);
    let write_start = (secs("write_start_secs") * rate).round() as usize;
    let write_end = (secs("write_end_secs") * rate).round() as usize;

    let patched: Vec<i16> = WavReader::open(&out_wav)
        .expect("open patched wav")
        .samples::<i16>()
        .map(|s| s.expect("read sample"))
        .collect();
    assert_eq!(patched.len(), raw_a.len(), "splice is length-preserving");
    let changed: Vec<usize> = (0..raw_a.len())
        .filter(|&i| patched[i] != raw_a[i])
        .collect();
    let (first, last) = (changed[0], *changed.last().expect("the splice changed A"));
    assert!(
        write_start <= first && last < write_end,
        "changed [{first}, {last}] must lie inside the write [{write_start}, {write_end})"
    );
    let slack = (0.03 * rate) as usize;
    assert!(
        first - write_start <= slack && write_end - 1 - last <= slack,
        "the write must be tight: changed [{first}, {last}] vs [{write_start}, {write_end})"
    );

    // The gap's first written frame, in A seconds: the B fill frame there is OFFSET_SECS later.
    let refine_start = placement["refine_start_frames"]
        .as_i64()
        .expect("a patched gap from repair characterize records its refine move");
    assert_ne!(refine_start, 0, "the soft lead must move the start edge");
    let a_start = gaps[0]["a_start_secs"].as_f64().expect("a_start_secs");
    let gap_start = a_start + refine_start as f64 / rate;
    let fill_source = secs("fill_source_start_secs");
    assert!(
        (fill_source - (gap_start + f64::from(OFFSET_SECS))).abs() < 0.05,
        "fill source {fill_source:.4} s must sit {OFFSET_SECS} s after gap start {gap_start:.4} s"
    );
}
