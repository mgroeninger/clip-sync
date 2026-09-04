//! End-to-end known answer for the whole-program A/B level offset.
//!
//! Tier: **integration**. Synthetic chirp pair (no ffmpeg, no licensed media).
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test cli_program_levels`
//!
//! The estimator's unit tests (`domain::program_levels`) feed `BlockLevel` values in directly, which
//! leaves everything between the container and the reduce untested: the decoder, the block-level
//! scanner, the dB conversion, the recipe-derived active threshold, and the mapping from A's
//! timeline onto B's. A flipped sign or a threshold compared in the wrong domain passes every one of
//! those tests. Here a pair that differs *only* by a known gain goes through the real binary and the
//! reported `offset_db` is checked against the gain that was written.
//!
//! Driven as a subprocess against `--format json` rather than by calling `ScanGaps` directly: the
//! serialization is part of what is being pinned, since these fields are consumed out of the JSON.
//!
//! Scope, so this is not later read as more than it is — the chirp is constant amplitude, so the
//! steadiness filter has nothing to reject, and 120 s sits inside a single 600 s chunk, so chunking
//! never engages. Estimator *behaviour* is the media tiers' job; this is wiring.

use clip_sync_repair_fixtures::gain_pair::{write_gain_pair, GainPair};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Gain written onto B. Negative deliberately: a boost on a fixture near full scale risks clipping,
/// which lowers measured RMS gain and raises spread — indistinguishable from estimator bias.
const GAIN_DB: f64 = -6.0;

/// 30 ms at 48 kHz — a third of a 100 ms scan block, so A's blocks straddle two B blocks and the
/// largest-overlap mapping has to choose. Deliberately not a tie: at exactly half a block the choice
/// would be a knife edge and the test would be flaky rather than meaningful.
const FRACTIONAL_PAD_SAMPLES: u64 = 1_440;

/// Run the repair binary over the pair and return the parsed report.
///
/// `min_fill_correlation` is floored because chirp shoulders either side of a 30 s hole do not
/// correlate across the seam; the fill gate is not what this test is about, and the level capture
/// runs in the scan half regardless of what the patch decides.
fn scan(pair: &GainPair, dir: &Path) -> Value {
    let config_path = dir.join("repair.toml");
    std::fs::write(
        &config_path,
        "[clip]\nclip_length = 60\n\n[repair]\nmin_fill_correlation = -1.0\nscan_both = false\n",
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_clip-sync-repair"))
        .args([
            pair.path_a.to_str().expect("path_a utf8"),
            pair.path_b.to_str().expect("path_b utf8"),
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

/// `scan.program_levels.offset`, insisting it is present.
///
/// Absence is the failure mode worth naming: B's level timeline is only scanned when an alignment
/// offset exists, so an alignment that does not land leaves this `None` and every downstream
/// assertion vacuous. A test that tolerated `None` here would pass while measuring nothing.
fn offset(report: &Value) -> &Value {
    let levels = report
        .get("scan")
        .and_then(|s| s.get("program_levels"))
        .expect("scan.program_levels is present");
    levels
        .get("offset")
        .filter(|v| !v.is_null())
        .expect("program_levels.offset is present — no offset means alignment did not land")
}

fn f64_at(value: &Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("offset.{key} is a number"))
}

fn usize_at(value: &Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("offset.{key} is an integer"))
}

#[test]
fn a_gain_scaled_pair_recovers_the_applied_gain() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pair = write_gain_pair(temp.path(), GAIN_DB, 0);
    let report = scan(&pair, temp.path());
    let offset = offset(&report);

    // The point of the whole tier: sign convention and dB conversion, end to end.
    assert!(
        (f64_at(offset, "offset_db") - pair.gain_db).abs() < 0.1,
        "offset_db {} should recover the written {} dB",
        f64_at(offset, "offset_db"),
        pair.gain_db
    );
    // A constant gain is a constant difference. Anything but a near-zero spread means the pairing is
    // drifting, which a correct median would hide.
    assert!(
        f64_at(offset, "spread_db") < 0.01,
        "a constant gain should give a near-zero spread, got {}",
        f64_at(offset, "spread_db")
    );
    assert!(offset["channels_comparable"].as_bool().expect("bool"));

    // Same predicate over the same timeline, now over a *decoded* one rather than fixture literals.
    let a_active = report["scan"]["program_levels"]["a"]["n_active"]
        .as_u64()
        .expect("a.n_active");
    assert_eq!(usize_at(offset, "n_active"), a_active);
    assert!(usize_at(offset, "n_steady") <= usize_at(offset, "n_active"));
    assert!(usize_at(offset, "n_pairs") <= usize_at(offset, "n_steady"));

    // Yield: the sides share a timeline, so nearly every steady block should find a B partner. A low
    // yield here would mean the mapping or the fail-closed prefix bound is rejecting good blocks.
    assert!(
        usize_at(offset, "n_pairs") + 5 >= usize_at(offset, "n_steady"),
        "expected near-total B coverage, got {} pairs from {} steady",
        usize_at(offset, "n_pairs"),
        usize_at(offset, "n_steady")
    );

    // The only quiet blocks are the digitally silent core, and digital zero is excluded from both
    // per-side populations — so a floor reading would mean the sentinel had leaked into one.
    assert!(
        report["scan"]["program_levels"]["a"]
            .get("floor_db")
            .is_none(),
        "digital silence is not a floor measurement"
    );
}

#[test]
fn a_sub_block_shift_still_recovers_the_gain() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pair = write_gain_pair(temp.path(), GAIN_DB, FRACTIONAL_PAD_SAMPLES);
    let report = scan(&pair, temp.path());

    // Confirm the shift is real before reading anything off it: at a zero offset the mapping is the
    // identity and this test would be a duplicate of the one above.
    let aligned = report["scan"]["alignment"]["recommended_offset_secs"]
        .as_f64()
        .expect("an alignment offset");
    assert!(
        (aligned - pair.pad_secs).abs() < 0.01,
        "alignment {aligned} should find the {} s pad",
        pair.pad_secs
    );

    let offset = offset(&report);
    assert!(
        (f64_at(offset, "offset_db") - pair.gain_db).abs() < 0.1,
        "a sub-block misregistration should not move the gain: got {}, wrote {}",
        f64_at(offset, "offset_db"),
        pair.gain_db
    );
    // Spread is no longer ~0 — each A block is now compared against a B block covering a slightly
    // different span of the chirp — but it must stay far below the tolerance the offset is read at.
    assert!(
        f64_at(offset, "spread_db") < 0.05,
        "sub-block spread {} is larger than a misregistration of this size can explain",
        f64_at(offset, "spread_db")
    );
}
