use std::path::PathBuf;
use std::time::Duration;

use clip_sync::{LogLevel, ProgressMode};
use clip_sync_cli::infrastructure::config::{load_app_config, AppConfig, OutputFormat};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analyzer.toml")
}

#[test]
fn analyzer_fixture_deserializes_and_validates() {
    let config = load_app_config(Some(&fixture_path())).expect("load fixture");

    assert_eq!(config.align.clip.clip_length, Duration::from_secs(900));
    assert_eq!(config.align.clip.num_clips, 1);
    assert!(config.align.clip.normalize_loudness);
    assert!(config.align.clip.trim_silence);
    assert_eq!(config.align.alignment.min_match_score, 0.3);
    assert!(config.align.alignment.refine_offset_with_pcm);
    assert!(!config.align.alignment.refine_offset_high_rate);
    assert_eq!(config.align.alignment.high_rate_refine_secs, 3);
    assert!(!config.align.alignment.try_all_tracks);
    assert!(!config.align.validation.check_clip_repetition);
    assert_eq!(config.align.validation.min_repetition_confidence, 0.5);
    assert_eq!(config.output.format, OutputFormat::Human);
    assert!(!config.output.show_diagnostics);
    assert_eq!(config.logging.level, LogLevel::Warn);
    assert_eq!(config.logging.progress, ProgressMode::Auto);

    config.validate().expect("fixture should be valid");
}

#[test]
fn analyzer_fixture_roundtrips_through_toml() {
    let config = load_app_config(Some(&fixture_path())).expect("load fixture");
    let serialized = toml::to_string(&config).expect("serialize AppConfig");
    let reparsed: AppConfig = toml::from_str(&serialized).expect("re-parse serialized config");
    assert_eq!(config, reparsed);
}
