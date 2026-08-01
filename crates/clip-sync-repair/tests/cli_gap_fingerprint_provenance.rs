//! CLI `--gap-fingerprints` source-provenance smoke dump on a lossless A/B pair.
//!
//! Tier: **integration**. Runs the real binary end-to-end — align → scan → `dump_gap_fingerprints`
//! → `corpus.json` — so the provenance in the dump comes from an actual container probe rather than
//! a hand-built `SourceDescriptor`. That is the one half of Track A's definition of done the unit
//! tests structurally cannot cover: they all inject the descriptor they then assert on.
//!
//! PR: **yes** — `pr-repair` (requires `--features calibration`; `--gap-fingerprints` does not exist
//! without it, so the whole file is compiled out otherwise).
//!
//! Run: `cargo test -p clip-sync-repair --features calibration --test cli_gap_fingerprint_provenance`
//!
//! Licensing-safe: the media is synthesized here, so nothing about real content enters the tree.

#![cfg(feature = "calibration")]

use clip_sync_repair_fixtures::lossless_silence_pair::write_lossless_silence_pair;
use serde_json::Value;
use std::process::Command;

/// Read `source.<side>_source.<field>`, failing with the path that was missing rather than a bare
/// `None` — an absent key and a null are different bugs and the message should say which.
fn field<'a>(corpus: &'a Value, side: &str, name: &str) -> &'a Value {
    corpus
        .get("source")
        .and_then(|s| s.get(format!("{side}_source")))
        .and_then(|s| s.get(name))
        .unwrap_or_else(|| panic!("corpus.json has no source.{side}_source.{name}"))
}

#[test]
fn gap_fingerprint_dump_records_probed_source_provenance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pair = write_lossless_silence_pair(temp.path());
    let dump_dir = temp.path().join("fingerprints");

    let config_path = temp.path().join("repair.toml");
    // Chirp shoulders either side of a 30 s hole do not correlate across the seam; the gate is not
    // what this test is about, and a blocked fill still produces a characterized corpus.
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
            pair.path_a.to_str().expect("path_a utf8"),
            pair.path_b.to_str().expect("path_b utf8"),
            "--config",
            config_path.to_str().expect("config utf8"),
            "--gap-fingerprints",
            dump_dir.to_str().expect("dump dir utf8"),
            "--min-gap-ms",
            "25000",
            "--no-scan-both",
        ])
        .output()
        .expect("run clip-sync-repair");
    assert!(
        output.status.success(),
        "CLI should exit 0.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let corpus: Value = serde_json::from_str(
        &std::fs::read_to_string(dump_dir.join("corpus.json")).expect("read corpus.json"),
    )
    .expect("parse corpus.json");

    // The pair is comparable (equal channel counts), so gaps must actually have been measured —
    // otherwise every assertion below would pass over an empty corpus.
    assert_eq!(
        corpus.get("source").and_then(|s| s.get("incomparable")),
        None,
        "equal native_channels must not trip the refuse gate"
    );
    assert!(
        !corpus["gaps"].as_array().expect("gaps array").is_empty(),
        "the 30 s silent core should have been detected as a gap"
    );

    // 1. Both sides report the codec **family**, `"pcm"` — not a per-id token. The two sides are
    //    different Symphonia codec ids here (s16le `0x108` vs s24le `0x104`), and before
    //    `codec_name` grew a linear-PCM arm they surfaced as exactly those hex strings. Equality is
    //    therefore the assertion with teeth: it pins that depth is *not* leaking into the codec axis,
    //    where it would split one logical population into two census buckets while restating what
    //    `bit_depth` already says two lines below.
    for side in ["a", "b"] {
        assert_eq!(
            field(&corpus, side, "codec"),
            "pcm",
            "{side}_source.codec should be the family token"
        );
    }

    // 2. Bit depth is read per side, and the two sides differ — a probe that reported one side's
    //    reading for both would pass a same-depth fixture.
    assert_eq!(field(&corpus, "a", "bit_depth"), "s16");
    assert_eq!(field(&corpus, "b", "bit_depth"), "s24");

    // 3. Native rates are each side's own, not the rate everything was measured at.
    assert_eq!(field(&corpus, "a", "native_sample_rate"), pair.rate_a);
    assert_eq!(field(&corpus, "b", "native_sample_rate"), pair.rate_b);

    // 4. `sample_rate` is the measurement rate (A's) on *both* sides, which is what makes
    //    `was_resampled()` — `native_sample_rate != sample_rate` — read false for A and true for B.
    //    Asserting both halves is the point: a corpus where B silently kept its native rate would
    //    still have the right `native_sample_rate` and the wrong answer to "was this resampled?".
    for side in ["a", "b"] {
        assert_eq!(
            field(&corpus, side, "sample_rate"),
            pair.rate_a,
            "{side}_source.sample_rate should be the measurement rate, not the native one"
        );
    }

    // 5. Channels: equal here by construction (the refuse gate's precondition), so this pins that
    //    the field is populated at all rather than distinguishing the sides.
    for side in ["a", "b"] {
        assert_eq!(field(&corpus, side, "native_channels"), pair.channels);
        assert_eq!(field(&corpus, side, "channels"), pair.channels);
    }
}
