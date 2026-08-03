//! CLI `--gap-listen` end-to-end wiring smoke test.
//!
//! Tier: **integration**. Runs the real binary — align → scan → `gap_listen` → WAVs — so it covers
//! the one seam `gap_listen_integration` structurally cannot: **`composition.rs`'s dispatch**.
//! Those tests call `run_gap_listen` directly, so every one of them would still pass if
//! `--gap-listen` were never routed from `run_inner`, or if the returned `PatchAudioResult` were
//! dropped on the floor instead of folded into `RepairRunOutcome.patch_result`.
//!
//! PR: **yes** — `pr-repair` (requires `--features calibration`; `--gap-listen` does not exist
//! without it, so the whole file is compiled out otherwise).
//!
//! Run: `cargo test -p clip-sync-repair --features calibration --test cli_gap_listen`
//!
//! Licensing-safe: the media is synthesized here, so nothing about real content enters the tree.
//! The WAVs it writes live in a `tempfile` dir that is deleted with the test.

#![cfg(feature = "calibration")]

use clip_sync::testing::audio_fixtures::write_offset_chirp_wav_pair;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECS: u32 = 120;
const OFFSET_SECS: u32 = 3;
const SILENT_START_SECS: u32 = 30;
const SILENT_END_SECS: u32 = 60;

/// Mute `[start_secs, end_secs)` in place, so A has a gap where B still has audio.
///
/// The chirp pair is the fixture of record for CLI-level patch tests
/// (`cli_wav_integration::cli_scan_and_wav_writes_patched_output`) for two reasons that both matter
/// here: a chirp aligns unambiguously (a pure sine is periodic, so the offset would be), and
/// zeroing only A leaves B a real donor. `lossless_silence_pair` — the fixture the sibling
/// `cli_gap_fingerprint_provenance` uses — is silent on *both* sides, so its gap comes back
/// `unfillable` with no donor: it exercises the dump but can never reach a patch verdict.
fn zero_wav_segment(path: &Path, sample_rate: u32, start_secs: u32, end_secs: u32) {
    let mut reader = WavReader::open(path).expect("open wav");
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .map(|sample| sample.expect("read sample"))
        .collect();

    let start = (u64::from(sample_rate) * u64::from(start_secs)) as usize;
    let end = (u64::from(sample_rate) * u64::from(end_secs)) as usize;
    let mut muted = samples;
    for sample in muted.iter_mut().take(end).skip(start) {
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

/// Stems of every `.wav` in `dir`, sorted. Empty if the directory was never created.
fn wav_stems(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stems: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".wav"))
        .collect();
    stems.sort();
    stems
}

/// The whole point of this file: `--gap-listen` is reachable from the CLI, writes its clips, and
/// the production verdicts behind them survive into the ordinary gap report.
#[test]
fn gap_listen_is_wired_through_composition_and_reports_its_verdicts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (path_a, path_b) =
        write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);
    zero_wav_segment(&path_a, SAMPLE_RATE, SILENT_START_SECS, SILENT_END_SECS);
    let dump_dir = temp.path().join("fingerprints");
    let wav_dir = temp.path().join("listen");

    let config_path = temp.path().join("repair.toml");
    // `min_fill_correlation = -1.0` as in `cli_wav_integration`: chirp segments need not correlate
    // across the seam, and the gate is not what this test is about. Keeping the fill unblocked is
    // the point — it drives the run down the *patched*-clip branch, which is the branch that proves
    // a real patch verdict reached the report.
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
    let output = Command::new(bin)
        .args([
            path_a.to_str().expect("path_a utf8"),
            path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--gap-fingerprints",
            dump_dir.to_str().expect("dump dir utf8"),
            "--gap-listen",
            wav_dir.to_str().expect("wav dir utf8"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ])
        .output()
        .expect("run clip-sync-repair");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "CLI should exit 0.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // 1. The dump still happens. `--gap-listen` is a side channel on `--gap-fingerprints`, not a
    //    replacement for it, so a run that wrote clips but no corpus has broken the join.
    let corpus: Value = serde_json::from_str(
        &std::fs::read_to_string(dump_dir.join("corpus.json")).expect("read corpus.json"),
    )
    .expect("parse corpus.json");
    let gaps = corpus["gaps"].as_array().expect("gaps array");
    assert!(
        !gaps.is_empty(),
        "the 30 s muted span in A should have been detected as a gap"
    );

    // 2. All three clips were written, into the directory the flag named. This is the assertion
    //    that fails if `composition.rs` never dispatches to `gap_listen` — the unit tests call
    //    `run_gap_listen` directly and so cannot see that seam at all.
    let stems = wav_stems(&wav_dir);
    for suffix in ["_a_surround.wav", "_b_surround.wav", "_a_patched.wav"] {
        assert!(
            stems.iter().any(|n| n.ends_with(suffix)),
            "a patched listen run must write {suffix}, got {stems:?}\nstdout:\n{stdout}"
        );
    }

    // 3. Every clip stem joins back to a per-gap JSON in the corpus dir. Checked here and not only
    //    in the unit test because the CLI is where the two directories are actually different
    //    paths — a stem built against the wrong root would still be self-consistent in-process.
    for stem in &stems {
        let entry = stem
            .rsplit_once("_a_surround.wav")
            .or_else(|| stem.rsplit_once("_b_surround.wav"))
            .or_else(|| stem.rsplit_once("_a_patched.wav"))
            .map(|(head, _)| head)
            .unwrap_or_else(|| panic!("unrecognized listen clip suffix: {stem}"));
        assert!(
            dump_dir.join(format!("{entry}.json")).exists(),
            "clip {stem} has no fingerprint sibling {entry}.json in the corpus dir"
        );
    }

    // 4. The verdicts survived. `run_gap_listen` returns the production `PatchAudioResult` and
    //    `run_inner` folds it into `RepairRunOutcome.patch_result` (§12); if that fold were dropped,
    //    the run would still write all three clips correctly and print a gap table reporting
    //    **0 repaired** — the exact "clips without reasons" failure the fold exists to prevent.
    //    Asserting the count and not merely that a table printed is what gives this teeth.
    assert!(
        stdout.contains("1 repaired"),
        "the production patch verdict must reach the gap table; a run that wrote a patched clip but \
         reported 0 repaired means the `patch_result` fold was dropped.\nstdout:\n{stdout}"
    );

    // 5. No output file is claimed. `--wav` / `--mux` are rejected on a listen run, so
    //    `output_written` is `None` and nothing may report a written deliverable.
    assert!(
        !stdout.contains("Wrote "),
        "a listen run writes no deliverable and must not claim one.\nstdout:\n{stdout}"
    );
}
