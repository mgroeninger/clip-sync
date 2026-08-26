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
    let master = master_wav(dir, source_id);
    let gained = variant(
        dir,
        &master,
        &format!("{gain_db}db"),
        &format!("volume={gain_db}dB"),
    );
    (master, gained)
}

/// Decode `source_id` to a mono WAV master under `dir`, once. A sweep re-uses one master across
/// every gain in the ladder; decoding 666 s of audio per sweep point is the dominant cost otherwise.
fn master_wav(dir: &Path, source_id: &str) -> PathBuf {
    let master = dir.join(format!("{source_id}_master.wav"));
    if master.is_file() {
        return master;
    }
    let sources = load_sources();
    let source = find_source(&sources, source_id);
    assert!(
        prepare_source_master_wav(&source_cache_path(source), &master, MASTER_RATE, None),
        "ffmpeg decode failed for {source_id}"
    );
    master
}

/// A B-side built from `master` by one ffmpeg audio filter, cached under `dir` by `label`.
fn variant(dir: &Path, master: &Path, label: &str, filter: &str) -> PathBuf {
    let stem = master
        .file_stem()
        .expect("master stem")
        .to_string_lossy()
        .to_string();
    let out = dir.join(format!("{stem}_{label}.wav"));
    if !out.is_file() {
        assert!(
            ffmpeg_util::filter_wav(master, &out, filter),
            "ffmpeg filter `{filter}` failed for {label}"
        );
    }
    out
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

/// `scan.program_levels.offset`, or `None` when the estimator abstained.
///
/// Distinct from [`offset`]: the sweeps below push the gain deep enough that B's quiet blocks fall
/// under the absolute active threshold and fewer than `MIN_PAIRS` survive. That abstention is a
/// *result* there — the depth at which the scalar stops being measurable — not a broken fixture.
fn try_offset(report: &Value) -> Option<Value> {
    report["scan"]["program_levels"]["offset"]
        .as_object()
        .map(|o| Value::Object(o.clone()))
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

// ---------------------------------------------------------------------------
// Trust-threshold calibration — docs/dev/TEMP-program-gain-plan.md § 4a
// ---------------------------------------------------------------------------
//
// The Tier 3 census (51 pairs, 2026-08-26) proved a single scalar exists on the corpus and set
// `program_gain_deadband_db`, but it could set nothing else: every corpus offset is inside 0.26 dB,
// so `spread_db`, chunk drift and the gain clamp never bind there. Those three only bind when the
// masters genuinely differ, and no such pair is on disk.
//
// The gap is not "how large is spread when a scalar exists" — Tier 3 answered that (median 0.668,
// max 2.412, robust drift max 1.291). It is the **other side**: how large do those statistics get
// when a scalar genuinely does *not* exist. A threshold needs both. These cases manufacture the
// negative side over real public-domain material with a known answer:
//
// | fixture              | a single scalar… | which statistic must see it |
// |----------------------|------------------|-----------------------------|
// | flat gain            | exists           | neither (the control)       |
// | lossy round trip     | exists           | neither — sizes codec noise |
// | `acompressor` on B   | does not exist   | `spread_db`                 |
// | time-ramped gain     | does not exist   | chunk drift                 |
//
// The two defects are deliberately separated: the compressor varies the offset with *level* at a
// constant program mean, the ramp varies it with *time* at a constant dynamic range. If one gate
// caught both, one of them would be redundant.

/// Flat-gain ladder. Runs past the point where the estimator abstains — the depth at which a
/// program offset stops being measurable at all is the ceiling any `max_program_gain_db` must
/// respect, since a gain the scan cannot measure is a gain the patch cannot apply.
const SWEEP_GAINS_DB: [f64; 6] = [-1.0, -6.0, -12.0, -24.0, -40.0, -55.0];

/// Recovery tolerance for a known flat gain. Tier 1 holds 0.01 on a chirp; real material and a
/// re-encode on one side widen it, and the existing cases already use 0.1–0.2.
const RECOVERY_TOL_DB: f64 = 0.2;

/// Ramp depths for the time-varying fixture, applied linearly from 0 dB at t=0. A ladder, not one
/// point: a single depth says only "this defect is caught", and the number a threshold needs is the
/// *shallowest* defect it still catches.
const RAMP_SWEEP_DB: [f64; 2] = [3.0, 12.0];

/// Compression ratios for the level-varying fixture, at a fixed threshold. Same argument as
/// [`RAMP_SWEEP_DB`]: the gate's detection floor is the reading a threshold has to sit under.
const COMPRESSOR_RATIOS: [u32; 3] = [2, 4, 8];

/// Widest `spread_db` on the Tier 3 corpus (51 pairs, 2026-08-26) — a pair whose offset is
/// nonetheless a single scalar. Any spread gate must sit above this, so a defect it can catch must
/// read above it too.
const CORPUS_MAX_SPREAD_DB: f64 = 2.412;

/// Widest robust chunk drift on the same corpus, on pairs whose offset is likewise constant.
const CORPUS_MAX_ROBUST_DRIFT_DB: f64 = 1.291;

/// The value that actually ships, so this file measures the gate rather than the calibration that
/// preceded it. It sits above [`CORPUS_MAX_SPREAD_DB`] by construction; the assertion below is that
/// a real dynamics mismatch reads above *this*, not merely above the corpus.
const SHIPPED_MAX_SPREAD_DB: f64 = clip_sync_repair::domain::DEFAULT_PROGRAM_GAIN_MAX_SPREAD_DB;

/// dB-to-neper conversion, so the ramp expression needs no comma (a comma inside an ffmpeg filter
/// argument separates filters, and escaping it through `Command` args is a portability trap).
const LN10_OVER_20: f64 = 0.115_129_254_6;

/// One sweep point, as the gate in § 4 of the plan would see it.
struct Reading {
    offset_db: f64,
    spread_db: f64,
    n_pairs: u64,
    /// `None` when the program spans fewer than two chunks — undefined, not zero.
    drift: Option<Drift>,
}

/// Chunk drift, computed exactly as `scripts/measure/census-program-levels.ps1` does, so a number
/// here is comparable with the Tier 3 table.
struct Drift {
    n_chunks: usize,
    /// max minus min over every chunk. A range statistic, so the least-populated chunk sets it.
    raw_db: f64,
    /// Same over chunks holding at least half the median chunk population. `None` when fewer than
    /// two chunks clear that bar. This is the statistic the plan's § 4 gate must read.
    robust_db: Option<f64>,
}

fn chunk_drift(offset: &Value) -> Option<Drift> {
    let chunks = offset["chunks"].as_array()?;
    if chunks.len() < 2 {
        return None;
    }
    let range = |values: &[f64]| {
        values.iter().cloned().fold(f64::MIN, f64::max)
            - values.iter().cloned().fold(f64::MAX, f64::min)
    };
    let chunk_db = |chunk: &Value| chunk["offset_db"].as_f64().expect("chunk offset_db");
    let chunk_pairs = |chunk: &Value| chunk["n_pairs"].as_u64().expect("chunk n_pairs");

    let dbs: Vec<f64> = chunks.iter().map(chunk_db).collect();
    let mut counts: Vec<u64> = chunks.iter().map(chunk_pairs).collect();
    counts.sort_unstable();
    let bar = counts[(counts.len() - 1) / 2] as f64 / 2.0;
    let solid: Vec<f64> = chunks
        .iter()
        .filter(|chunk| chunk_pairs(chunk) as f64 >= bar)
        .map(chunk_db)
        .collect();

    Some(Drift {
        n_chunks: chunks.len(),
        raw_db: range(&dbs),
        robust_db: (solid.len() >= 2).then(|| range(&solid)),
    })
}

/// Print one calibration row and return it. `None` is an abstention, which every caller must decide
/// about explicitly rather than read as a zero.
fn read_point(label: &str, expected_db: Option<f64>, report: &Value) -> Option<Reading> {
    let Some(offset) = try_offset(report) else {
        eprintln!("{label:<26} ABSTAINED (fewer than MIN_PAIRS blocks paired)");
        return None;
    };
    let reading = Reading {
        offset_db: num(&offset, "offset_db"),
        spread_db: num(&offset, "spread_db"),
        n_pairs: count(&offset, "n_pairs"),
        drift: chunk_drift(&offset),
    };
    let error = match expected_db {
        Some(expected) => format!("{:+.3}", reading.offset_db - expected),
        None => "      -".to_string(),
    };
    let (chunks, raw, robust) = match &reading.drift {
        Some(drift) => (
            drift.n_chunks.to_string(),
            format!("{:.3}", drift.raw_db),
            drift
                .robust_db
                .map_or("     -".to_string(), |value| format!("{value:.3}")),
        ),
        None => ("1".to_string(), "     -".to_string(), "     -".to_string()),
    };
    eprintln!(
        "{label:<26} offset {:+8.3}  err {error:>7}  spread {:7.3}  n {:5}  chunks {chunks:>2}  \
         drift {raw:>6}  drift* {robust:>6}",
        reading.offset_db, reading.spread_db, reading.n_pairs,
    );
    Some(reading)
}

/// Duration of a mono 16-bit WAV from its size. ffmpeg's WAV header is a few dozen bytes against
/// 96 kB/s of payload, so the header approximation costs well under 10 ms — irrelevant to a ramp
/// slope, and nothing else here reads this.
fn wav_duration_secs(path: &Path) -> f64 {
    const HEADER_BYTES: u64 = 44;
    let len = std::fs::metadata(path).expect("wav metadata").len();
    len.saturating_sub(HEADER_BYTES) as f64 / (2.0 * f64::from(MASTER_RATE))
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn calibration_flat_gain_is_the_scalar_exists_control() {
    if !ready(GRIEG) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let master = master_wav(temp.path(), GRIEG);

    let mut worst_spread: f64 = 0.0;
    let mut deepest_measured: Option<f64> = None;
    let mut shallowest_abstention: Option<f64> = None;

    for gain_db in SWEEP_GAINS_DB {
        let gained = variant(
            temp.path(),
            &master,
            &format!("{gain_db}db"),
            &format!("volume={gain_db}dB"),
        );
        // Both polarities. Swapping the roles swaps which side carries the filter's rounding and,
        // more to the point, which side loses its quiet blocks to the absolute active threshold.
        // The plan's attenuating half (B louder than A, § 3) is only exercised by the second row.
        for (label, a, b, expected) in [
            (
                format!("grieg B{gain_db:+.0} dB"),
                &master,
                &gained,
                gain_db,
            ),
            (
                format!("grieg B{:+.0} dB", -gain_db),
                &gained,
                &master,
                -gain_db,
            ),
        ] {
            let report = scan(a, b, temp.path());
            match read_point(&label, Some(expected), &report) {
                Some(reading) => {
                    assert!(
                        (reading.offset_db - expected).abs() < RECOVERY_TOL_DB,
                        "{label}: offset_db {} should recover {expected}",
                        reading.offset_db
                    );
                    // A flat gain is the same number on every block, so the median absolute
                    // deviation about it is zero up to float noise. Any spread on this row is the
                    // estimator inventing one — which is what makes the compressor row below
                    // readable as material rather than as method.
                    assert!(
                        reading.spread_db < 0.05,
                        "{label}: spread {} on a constant-gain pair is estimator error",
                        reading.spread_db
                    );
                    worst_spread = worst_spread.max(reading.spread_db);
                    deepest_measured = Some(gain_db.abs().max(deepest_measured.unwrap_or(0.0)));
                }
                None => {
                    shallowest_abstention =
                        Some(gain_db.abs().min(shallowest_abstention.unwrap_or(f64::MAX)));
                }
            }
        }
    }

    eprintln!(
        "flat-gain control: worst spread {worst_spread:.4} dB; deepest measured {deepest_measured:?} dB; \
         shallowest abstention {shallowest_abstention:?} dB"
    );
    assert!(
        deepest_measured.is_some(),
        "the ladder must measure at least one gain, or the fixture is broken rather than the \
         estimator honest"
    );
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn calibration_lossy_b_sizes_the_codec_share_of_spread() {
    if !ready(GRIEG) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let master = master_wav(temp.path(), GRIEG);
    let gained = variant(temp.path(), &master, "6db", "volume=-6dB");

    // A lossy B is the corpus condition, not an edge case: donors are encoded, and usually not the
    // same way the reference was. This row separates the spread a codec puts on a pair whose scalar
    // *does* exist from the spread a genuine dynamics difference puts on one whose scalar does not
    // — the two things `max_spread_db` has to sit between.
    let encoded = temp.path().join("grieg_-6db.mp3");
    assert!(
        ffmpeg_util::encode_audio(&gained, &encoded, ffmpeg_util::EncodeFormat::Mp3),
        "mp3 encode failed"
    );
    let decoded = temp.path().join("grieg_-6db_mp3.wav");
    assert!(
        ffmpeg_util::decode_to_mono_wav(&encoded, &decoded, MASTER_RATE, None),
        "mp3 decode failed"
    );

    let report = scan(&master, &decoded, temp.path());
    let reading = read_point("grieg B-6 dB via mp3", Some(-6.0), &report)
        .expect("a lossy round trip must not cost the offset");
    assert!(
        (reading.offset_db + 6.0).abs() < RECOVERY_TOL_DB,
        "offset_db {} should still recover -6 dB through a codec",
        reading.offset_db
    );
    eprintln!("codec share of spread: {:.3} dB", reading.spread_db);
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn calibration_compressed_b_must_show_up_as_spread() {
    if !ready(GRIEG) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let master = master_wav(temp.path(), GRIEG);

    // B with its dynamics changed: loud blocks pulled down, quiet blocks left alone, so the A-to-B
    // difference is a *function of level* and no single scalar describes it. `offset_db` will still
    // report something — a median always does. The question this row answers is whether `spread_db`
    // makes that answer visibly untrustworthy, because if it does not, § 4's spread gate cannot be
    // calibrated at any value and the plan needs a different check.
    let mut detected: Option<u32> = None;
    for ratio in COMPRESSOR_RATIOS {
        let compressed = variant(
            temp.path(),
            &master,
            &format!("compressed{ratio}"),
            &format!("acompressor=threshold=-30dB:ratio={ratio}:attack=20:release=250"),
        );
        let report = scan(&master, &compressed, temp.path());
        let reading = read_point(&format!("grieg B compressed {ratio}:1"), None, &report)
            .expect("compressed B should still pair");
        if reading.spread_db > SHIPPED_MAX_SPREAD_DB && detected.is_none() {
            detected = Some(ratio);
        }
    }

    // The shipped refusal is only meaningful if a genuine dynamics difference lands clear of it.
    // Which ratios clear it is the calibration; that at least one does is the assertion.
    const {
        assert!(
            SHIPPED_MAX_SPREAD_DB > CORPUS_MAX_SPREAD_DB,
            "the shipped max_spread_db must sit above the widest corpus pair that a single scalar does describe, or it refuses real work"
        )
    };
    assert!(
        detected.is_some(),
        "no compression ratio in {COMPRESSOR_RATIOS:?} pushed spread past the shipped \
         max_spread_db ({SHIPPED_MAX_SPREAD_DB} dB), so the shipped spread refusal catches no \
         dynamics mismatch this fixture can produce"
    );
    eprintln!(
        "spread gate detection floor: {:?}:1 compression (ratios below it hide inside the corpus range)",
        detected
    );
}

#[test]
#[ignore = "needs fetched corpus sources and ffmpeg"]
fn calibration_ramped_b_must_show_up_as_chunk_drift() {
    if !ready(AMIS) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let single = master_wav(temp.path(), AMIS);
    // 666 s gives one full chunk and a 66 s tail. Doubling gives chunks of comparable weight, which
    // is what a drift *range* statistic needs before it means anything.
    let master = temp.path().join("amis_doubled.wav");
    assert!(
        ffmpeg_util::loop_wav(&single, &master, 2),
        "ffmpeg loop failed"
    );
    let duration_secs = wav_duration_secs(&master);
    eprintln!("amis doubled: {duration_secs:.1} s");

    // Control first: the same program at a flat gain must read as no drift, or the ramp rows below
    // measure the fixture rather than the defect.
    let flat = variant(temp.path(), &master, "6db", "volume=-6dB");
    let flat_reading = read_point(
        "amis doubled B-6 dB",
        Some(-6.0),
        &scan(&master, &flat, temp.path()),
    )
    .expect("flat gain must measure");
    let flat_drift = flat_reading
        .drift
        .as_ref()
        .expect("a doubled 666 s program spans more than one 600 s chunk");
    assert!(
        flat_drift.n_chunks >= 3,
        "expected at least three chunks from {duration_secs:.0} s, got {}",
        flat_drift.n_chunks
    );
    let flat_robust = flat_drift.robust_db.expect("flat robust drift");

    // The defect: gain ramping linearly from 0 dB to the swept depth across the program. Constant
    // dynamic range, so this is drift's case — though the ramp also varies *within* a chunk, so
    // spread moves too and the two statistics are not orthogonal. Report both.
    let mut detected: Option<f64> = None;
    for ramp_db in RAMP_SWEEP_DB {
        let ramped = variant(
            temp.path(),
            &master,
            &format!("ramped{ramp_db}"),
            &format!("volume=eval=frame:volume=exp(-{LN10_OVER_20}*{ramp_db}*t/{duration_secs})"),
        );
        let reading = read_point(
            &format!("amis doubled B ramp {ramp_db:.0} dB"),
            None,
            &scan(&master, &ramped, temp.path()),
        )
        .expect("ramped B should still pair");
        let drift = reading
            .drift
            .as_ref()
            .expect("the ramped pair spans the same chunks as its control");
        let robust = drift.robust_db.expect("ramped robust drift");
        eprintln!(
            "  ramp {ramp_db} dB over {duration_secs:.0} s: raw drift {:.3}, robust {robust:.3}, \
             spread {:.3}  (flat control robust {flat_robust:.3})",
            drift.raw_db, reading.spread_db
        );
        if robust > CORPUS_MAX_ROBUST_DRIFT_DB && detected.is_none() {
            detected = Some(ramp_db);
        }
    }

    // Same argument as the spread ladder: a drift gate exists only if a genuine time-varying offset
    // lands clear of the corpus's widest constant-offset pair.
    assert!(
        detected.is_some(),
        "no ramp in {RAMP_SWEEP_DB:?} dB pushed robust drift past the corpus maximum \
         ({CORPUS_MAX_ROBUST_DRIFT_DB} dB) on a pair whose offset is constant"
    );
    eprintln!(
        "drift gate detection floor: {:?} dB of ramp across a {duration_secs:.0} s program",
        detected
    );
}
