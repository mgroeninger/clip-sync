//! Repair TOML config roundtrip.
//!
//! Tier: **integration**. Loads `tests/fixtures/repair.toml` through `load_repair_app_config`.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test config_roundtrip`

use std::path::PathBuf;
use std::time::Duration;

use clip_sync::LogLevel;
use clip_sync_repair::infrastructure::config::{load_repair_app_config, RepairAppConfig};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repair.toml")
}

#[test]
fn repair_fixture_deserializes_and_validates() {
    let config = load_repair_app_config(Some(&fixture_path())).expect("load fixture");

    assert_eq!(config.align.clip.clip_length, Duration::from_secs(900));
    assert_eq!(config.align.clip.num_clips, 2);
    assert_eq!(config.align.alignment.min_match_score, 0.5);
    assert!(config.align.alignment.refine_offset_with_pcm);
    assert!(config.align.alignment.refine_offset_high_rate);
    assert!(!config.align.alignment.require_consistent_offsets);
    assert!(!config.align.alignment.try_all_tracks);
    assert_eq!(config.logging.level, LogLevel::Warn);
    assert_eq!(config.repair.min_gap_ms, 1000);
    assert_eq!(config.repair.silence_peak_fraction, 0.01);
    assert_eq!(config.repair.scan_block_ms, 250);
    // TOML takes the normalized amplitude, not the CLI's 0-32767 operator scale (F3).
    assert!((config.repair.absolute_silence_rms - 0.001007).abs() < 1e-9);
    assert_eq!(config.repair.decode_chunk_secs, 10);
    assert_eq!(config.repair.min_fill_correlation, 0.35);
    assert_eq!(config.repair.crossfade_ms, 10);
    assert!(config.repair.dry_run);
    assert_eq!(config.repair.output.video_codec, "copy");
    assert_eq!(config.repair.output.audio_codec, "aac");
    assert_eq!(config.repair.output.mux_audio_bitrate, "match_min");

    config
        .align
        .validate()
        .expect("align config should be valid");
    config
        .repair
        .validate()
        .expect("repair config should be valid");
}

#[test]
fn repair_fixture_reports_no_unknown_keys() {
    // Guards against false positives from the unknown-key detector on the full
    // repair config surface (incl. nested [repair.output]) — e.g. a future
    // `skip_serializing_if` on an accepted field would make it look "unknown".
    let raw = std::fs::read_to_string(fixture_path()).expect("read fixture");
    let config: RepairAppConfig = toml::from_str(&raw).expect("parse fixture");
    let unknown = clip_sync::unknown_toml_keys(&raw, &config);
    assert!(
        unknown.is_empty(),
        "valid fixture keys must not be flagged as unknown: {unknown:?}"
    );
}

#[test]
fn repair_config_flags_a_misspelled_repair_key() {
    let raw = "[repair]\nmin_gap_mss = 1000\n";
    let config: RepairAppConfig = toml::from_str(raw).expect("parse");
    assert_eq!(
        clip_sync::unknown_toml_keys(raw, &config),
        vec!["repair.min_gap_mss".to_string()]
    );
}

#[test]
fn repair_fixture_roundtrips_through_toml() {
    let mut config = load_repair_app_config(Some(&fixture_path())).expect("load fixture");
    // `profile_field_mask` is runtime load metadata (#[serde(skip)]); clear it so the
    // value round-trip compares equal to a bare TOML deserialize.
    config.repair.profile_field_mask = Default::default();
    let serialized = toml::to_string(&config).expect("serialize RepairAppConfig");
    let reparsed: RepairAppConfig =
        toml::from_str(&serialized).expect("re-parse serialized config");
    assert_eq!(config, reparsed);
}
