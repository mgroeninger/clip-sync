//! Tier 2 of the program-level validation ladder: a known gain over **real** audio.
//!
//! Tier 1 (`cli_program_levels.rs`) proves the wiring on a synthetic chirp, and that is all it can
//! prove — a constant-amplitude chirp is maximally steady, so the steadiness filter never rejects
//! anything, and at 120 s it sits inside a single 600 s chunk, so chunking never engages. This tier
//! puts a known answer on material that has neither property.
//!
//! Media are the repo's SHA-pinned public-domain / CC masters from `tests/corpus/sources.toml`
//! (attribution in `tests/corpus/THIRD_PARTY_AUDIO.md`) — not licensed corpus media, so these cases
//! are committed and reproducible and the sources may be named. They are gitignored and ffmpeg is
//! optional, so every case skips loudly when its inputs are missing, and the whole file is
//! `#[ignore]` by default:
//!
//! ```text
//! scripts/fetch_corpus_sources.ps1
//! cargo test -p clip-sync-repair --test corpus_program_levels -- --ignored --nocapture
//! ```
//!
//! Helpers are shared from `clip-sync` rather than reimplemented: `corpus_sources` owns the manifest
//! reader, the pinned cache path and the readiness guard, and `ffmpeg_util` owns the ffmpeg wrappers.
//! A local copy would fork the manifest reader — two readers of one `sources.toml` and its hashes.

use clip_sync::testing::corpus_sources::{
    find_source, load_sources, prepare_source_master_wav, source_cache_path, source_ready,
};
use clip_sync::testing::ffmpeg_util;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lossless, 154 s, wide orchestral dynamics. The clean known-answer case: no codec noise can reach
/// `spread_db`, so whatever spread appears is the estimator's.
const GRIEG: &str = "musopen_grieg_mountain_king";

/// 666 s — the only source in the repo longer than `CHUNK_SECS` (600 s), and therefore the only one
/// that can exercise chunking end to end. Lossy, so its spread carries generation loss too.
const AMIS: &str = "wikimedia_martin_amis_interview_1990";

/// Decode rate for the masters. Full bandwidth: the point of this tier is real material, and
/// downsampling to a corpus-sized rate would discard the transients the steadiness filter exists for.
const MASTER_RATE: u32 = 48_000;

/// Primary gain under test. Negative deliberately — see `gain_wav`'s note on clipping.
const GAIN_DB: f64 = -6.0;

/// Both inputs this tier needs, or a reason it cannot run.
fn ready(source_id: &str) -> bool {
    if !ffmpeg_available_checked() {
        eprintln!("SKIP: ffmpeg not on PATH");
        return false;
    }
    if !source_ready(source_id) {
        eprintln!("SKIP: {source_id} not fetched — run scripts/fetch_corpus_sources.ps1");
        return false;
    }
    true
}

fn ffmpeg_available_checked() -> bool {
    ffmpeg_util::ffmpeg_available()
}

/// Decode `source_id` to a mono WAV master, then write B as that master at `gain_db`.
///
/// A is the master itself rather than a re-encode of it: one fewer transform on the reference side,
/// so on a lossless source the only difference between the sides is the gain.
fn gained_pair(dir: &Path, source_id: &str, gain_db: f64) -> (PathBuf, PathBuf) {
    let sources = load_sources();
    let source = find_source(&sources, source_id);
    let master = dir.join(format!("{source_id}_master.wav"));
    assert!(
        prepare_source_master_wav(&source_cache_path(source), &master, MASTER_RATE, None),
        "ffmpeg decode failed for {source_id}"
    );

    let gained = dir.join(format!("{source_id}_{gain_db}db.wav"));
    assert!(
        ffmpeg_util::gain_wav(&master, &gained, gain_db),
        "ffmpeg gain failed for {source_id}"
    );
    (master, gained)
}

/// Scan a pair through the real binary and return the parsed report.
fn scan(a: &Path, b: &Path, dir: &Path) -> Value {
    let config_path = dir.join("repair.toml");
    std::fs::write(
        &config_path,
        "[clip]\nclip_length = 60\n\n[repair]\nmin_fill_correlation = -1.0\nscan_both = false\n",
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_clip-sync-repair"))
        .args([
            a.to_str().expect("a utf8"),
            b.to_str().expect("b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--patch-only",
            "--format",
            "json",
        ])
        .output()
        .expect("run clip-sync-repair");
    assert!(
        output.status.success(),
        "repair failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is a JSON report")
}

/// `scan.program_levels.offset`, insisting it is present — B's level timeline is only scanned when
/// an alignment offset exists, so a `None` here would make every downstream assertion vacuous.
fn offset(report: &Value) -> Value {
    report["scan"]["program_levels"]["offset"]
        .as_object()
        .map(|o| Value::Object(o.clone()))
        .expect("program_levels.offset present — absent means alignment did not land")
}

fn num(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("offset.{key} is a number"))
}

fn count(value: &Value, key: &str) -> u64 {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("offset.{key} is an integer"))
}

/// The counters an operator wants to see on a real-media run: what the steadiness filter cost, and
/// how much of A found a B partner.
fn report_yields(label: &str, offset: &Value) {
    let (active, steady, pairs) = (
        count(offset, "n_active") as f64,
        count(offset, "n_steady") as f64,
        count(offset, "n_pairs") as f64,
    );
    eprintln!(
        "{label}: offset {:+.3} dB  spread {:.3} dB  n={}  steady/active {:.3}  pairs/steady {:.3}",
        num(offset, "offset_db"),
        num(offset, "spread_db"),
        pairs as u64,
        steady / active,
        pairs / steady,
    );
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn a_lossless_master_recovers_a_known_gain() {
    if !ready(GRIEG) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let (a, b) = gained_pair(temp.path(), GRIEG, GAIN_DB);
    let report = scan(&a, &b, temp.path());
    let offset = offset(&report);
    report_yields("grieg lossless", &offset);

    assert!(
        (num(&offset, "offset_db") - GAIN_DB).abs() < 0.1,
        "offset_db {} should recover {GAIN_DB} dB on a lossless master",
        num(&offset, "offset_db")
    );
    // Lossless in, gain out: nothing but the estimator can put spread here. Looser than Tier 1's
    // 0.01 because real material varies within a block and the sides are not sample-aligned after a
    // re-encode, but still far below anything that would question the scalar.
    assert!(
        num(&offset, "spread_db") < 0.2,
        "spread {} on a lossless constant-gain pair is estimator error, not material",
        num(&offset, "spread_db")
    );
    assert!(offset["channels_comparable"].as_bool().expect("bool"));

    // Real dynamics, so — unlike Tier 1's chirp — the steadiness filter should actually reject
    // something. If it rejects nothing, STEADY_TOLERANCE_DB is not engaging on real material.
    assert!(
        count(&offset, "n_steady") < count(&offset, "n_active"),
        "orchestral material should have transient-adjacent blocks for the filter to drop"
    );
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn a_source_longer_than_a_chunk_reports_per_chunk_offsets() {
    if !ready(AMIS) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let (a, b) = gained_pair(temp.path(), AMIS, GAIN_DB);
    let report = scan(&a, &b, temp.path());
    let offset = offset(&report);
    report_yields("amis lossy 666s", &offset);

    // The reason this source is in the tier at all: 666 s crosses CHUNK_SECS, so `chunks` is
    // exercised here and nowhere else in the test suite.
    let chunks = offset["chunks"].as_array().expect("chunks array");
    assert!(
        chunks.len() >= 2,
        "a 666 s source should span more than one 600 s chunk, got {} chunk(s)",
        chunks.len()
    );
    for chunk in chunks {
        let chunk_db = chunk["offset_db"].as_f64().expect("chunk offset_db");
        assert!(
            (chunk_db - GAIN_DB).abs() < 0.2,
            "chunk at {}s reported {chunk_db} dB, expected {GAIN_DB}",
            chunk["start_secs"]
        );
    }
    assert!(
        (num(&offset, "offset_db") - GAIN_DB).abs() < 0.1,
        "whole-program offset should agree with its chunks"
    );
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn attenuation_shrinks_the_paired_population() {
    if !ready(GRIEG) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");

    // The prediction under test, from § 4 of the plan: attenuating B pushes its quiet blocks below
    // the absolute active threshold while A's still pass, so those pairs drop out. Because a median
    // is unbiased under a flat gain regardless, `offset_db` recovering correctly is NOT evidence the
    // threshold works — this count is the only thing that tests it.
    let mut previous: Option<(f64, u64)> = None;
    for gain_db in [-3.0, -12.0, -30.0] {
        let (a, b) = gained_pair(temp.path(), GRIEG, gain_db);
        let report = scan(&a, &b, temp.path());
        let offset = offset(&report);
        report_yields(&format!("grieg {gain_db} dB"), &offset);

        let pairs = count(&offset, "n_pairs");
        assert!(
            (num(&offset, "offset_db") - gain_db).abs() < 0.2,
            "offset_db {} should still recover {gain_db} dB",
            num(&offset, "offset_db")
        );
        if let Some((previous_gain, previous_pairs)) = previous {
            assert!(
                pairs <= previous_pairs,
                "attenuating from {previous_gain} to {gain_db} dB should not *grow* the paired \
                 population: {previous_pairs} -> {pairs}"
            );
        }
        previous = Some((gain_db, pairs));
    }

    let (_, deepest) = previous.expect("at least one gain ran");
    eprintln!("deepest attenuation kept {deepest} pairs");
}
