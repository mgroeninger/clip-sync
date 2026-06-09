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
    assert_eq!(config.repair.decode_chunk_secs, 10);
    assert_eq!(config.repair.min_fill_correlation, 0.35);
    assert_eq!(config.repair.crossfade_ms, 10);
    assert!(config.repair.dry_run);
    assert_eq!(config.repair.output.video_codec, "copy");
    assert_eq!(config.repair.output.audio_codec, "aac");

    config
        .align
        .validate()
        .expect("align config should be valid");
    config.repair.validate().expect("repair config should be valid");
}

#[test]
fn repair_fixture_roundtrips_through_toml() {
    let config = load_repair_app_config(Some(&fixture_path())).expect("load fixture");
    let serialized = toml::to_string(&config).expect("serialize RepairAppConfig");
    let reparsed: RepairAppConfig = toml::from_str(&serialized).expect("re-parse serialized config");
    assert_eq!(config, reparsed);
}
