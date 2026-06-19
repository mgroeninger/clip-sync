use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use clip_sync::{AlignConfig, AppError, ConfigError, LoggingConfig};

use crate::infrastructure::mux_bitrate::parse_mux_audio_bitrate_policy;

/// Default clip count for repair alignment (start + end windows on long media).
pub const REPAIR_DEFAULT_NUM_CLIPS: u32 = 2;

fn default_repair_align_config() -> AlignConfig {
    let mut align = AlignConfig::default();
    align.clip.num_clips = REPAIR_DEFAULT_NUM_CLIPS;
    // Auto (default): query-reference when inputs differ greatly in length; symmetric otherwise.
    align.alignment.mode = clip_sync::AlignmentMode::Auto;
    // Keep start-clip offset for fill when end disagrees; drift is surfaced in the report.
    align.alignment.require_consistent_offsets = false;
    // Sub-sample offset refinement is worth the extra cost for repair accuracy.
    align.alignment.refine_offset_high_rate = true;
    align
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairConfig {
    /// Minimum silent window duration (ms) to include in the gap report.
    #[serde(default = "default_min_gap_ms")]
    pub min_gap_ms: u64,
    /// Fraction of peak amplitude below which a block is considered silent.
    #[serde(default = "default_silence_peak_fraction")]
    pub silence_peak_fraction: f32,
    /// Analysis block size (ms) when scanning for silent runs in decoded PCM.
    #[serde(default = "default_scan_block_ms")]
    pub scan_block_ms: u64,
    /// Decode chunk size (seconds) for sequential PCM scan — not gap detection granularity.
    #[serde(default = "default_decode_chunk_secs", alias = "scan_window_secs")]
    pub decode_chunk_secs: u64,
    /// Number of consecutive non-silent blocks to absorb before closing a silence run.
    /// Derived from `silence_hold_ms / scan_block_ms`; use `silence_hold_blocks()`.
    #[serde(default = "default_silence_hold_ms")]
    pub silence_hold_ms: u64,
    /// Absolute RMS floor (0–32767 scale) below which a block is always silent regardless of peak.
    /// Catches low-level codec noise in compressed-audio gaps. Set to 0 to disable.
    #[serde(default = "default_absolute_silence_rms")]
    pub absolute_silence_rms: f32,
    /// Also scan B's native timeline for silence to produce `gap_offset_agreement`.
    #[serde(default = "default_true")]
    pub scan_both: bool,
    /// Maximum |silence_offset − alignment_offset| (seconds) to count as agreement.
    #[serde(default = "default_gap_offset_tolerance_secs")]
    pub gap_offset_tolerance_secs: f64,
    /// Minimum normalized Pearson correlation at each gap seam (pre and post). Regions below
    /// this threshold on either seam are skipped during patch.
    #[serde(default = "default_min_fill_correlation")]
    pub min_fill_correlation: f32,
    /// Extra B audio (seconds) extracted on each side of the mapped gap for boundary alignment.
    #[serde(default = "default_fill_align_margin_secs")]
    pub fill_align_margin_secs: f64,
    /// Maximum slide (seconds) when searching for the best-matching B fill position.
    #[serde(default = "default_max_fill_align_adjustment_secs")]
    pub max_fill_align_adjustment_secs: f64,
    /// How far (seconds) to search in B for A's pre-gap border before local alignment.
    #[serde(default = "default_fill_border_search_secs")]
    pub fill_border_search_secs: f64,
    /// Minimum border template length (seconds) for discovery/correlation on short gaps.
    #[serde(default = "default_min_border_discovery_secs")]
    pub min_border_discovery_secs: f64,
    /// A-side only: exclude audio this close (seconds) to the dropout when building border templates.
    #[serde(default = "default_border_standoff_secs")]
    pub border_standoff_secs: f64,
    /// Gaps at or below this length use mean(pre, post) correlation for the fill gate.
    #[serde(default = "default_short_gap_mean_correlation_secs")]
    pub short_gap_mean_correlation_secs: f64,
    /// How far B fill length may differ from A's scanned gap when locating the post-border.
    #[serde(default = "default_fill_length_slack_secs")]
    pub fill_length_slack_secs: f64,
    /// Seam correlation window (seconds) for fine align slide search and the fill gate.
    #[serde(default = "default_fill_seam_search_secs")]
    pub fill_seam_search_secs: f64,
    /// Seconds of A audio on each side of the gap used to build the structure signature.
    #[serde(default = "default_gap_signature_context_secs")]
    pub gap_signature_context_secs: f64,
    /// Bin width (milliseconds) for active/silent structure signatures.
    #[serde(default = "default_gap_signature_bin_ms")]
    pub gap_signature_bin_ms: u64,
    /// Minimum active/silent pattern match score (0–1) at each seam before waveform gate.
    #[serde(default = "default_min_structure_match_score")]
    pub min_structure_match_score: f32,
    /// Both structure seam scores must meet this to skip the waveform Pearson gate.
    #[serde(default = "default_strong_structure_trust")]
    pub strong_structure_trust: f64,
    /// When true, always run the waveform Pearson seam gate even when structure scores meet
    /// `strong_structure_trust`.
    #[serde(default)]
    pub disable_structure_trust: bool,
    /// In the waveform gate path, soften Pearson threshold when structure scores meet this.
    #[serde(default = "default_partial_structure_waveform_soften")]
    pub partial_structure_waveform_soften: f64,
    /// Crossfade duration at gap boundaries (ms).
    #[serde(default = "default_crossfade_ms")]
    pub crossfade_ms: u64,
    /// Normalize fill segment loudness to match A's border.
    #[serde(default = "default_true")]
    pub normalize_fill: bool,
    /// Window size (seconds) for computing A's border RMS.
    #[serde(default = "default_normalize_window_secs")]
    pub normalize_window_secs: f64,
    /// Maximum gain (dB) applied during normalization.
    #[serde(default = "default_max_fill_gain_db")]
    pub max_fill_gain_db: f64,
    /// Output configuration (paths for writing repaired audio/video).
    #[serde(default)]
    pub output: RepairOutputConfig,
    /// Dry-run: scan and report only; do not write any output files.
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// When query-reference alignment is used, only treat gaps inside the mapped B coverage
    /// region as fillable (gaps outside are still reported).
    #[serde(default = "default_true")]
    pub limit_fill_to_mapped_region: bool,
}

fn default_min_gap_ms() -> u64 {
    1_000
}
fn default_silence_peak_fraction() -> f32 {
    0.01
}
fn default_scan_block_ms() -> u64 {
    250
}
fn default_decode_chunk_secs() -> u64 {
    10
}
fn default_silence_hold_ms() -> u64 {
    500
}
fn default_absolute_silence_rms() -> f32 {
    33.0
}
fn default_true() -> bool {
    true
}
fn default_gap_offset_tolerance_secs() -> f64 {
    0.5
}
fn default_min_fill_correlation() -> f32 {
    0.35
}
fn default_fill_align_margin_secs() -> f64 {
    1.0
}
fn default_max_fill_align_adjustment_secs() -> f64 {
    0.5
}
fn default_fill_border_search_secs() -> f64 {
    30.0
}
fn default_min_border_discovery_secs() -> f64 {
    2.0
}
fn default_border_standoff_secs() -> f64 {
    0.35
}
fn default_short_gap_mean_correlation_secs() -> f64 {
    2.0
}
fn default_fill_length_slack_secs() -> f64 {
    5.0
}
fn default_fill_seam_search_secs() -> f64 {
    0.25
}
fn default_gap_signature_context_secs() -> f64 {
    3.0
}
fn default_gap_signature_bin_ms() -> u64 {
    50
}
fn default_min_structure_match_score() -> f32 {
    0.55
}
fn default_strong_structure_trust() -> f64 {
    0.90
}
fn default_partial_structure_waveform_soften() -> f64 {
    0.85
}
fn default_crossfade_ms() -> u64 {
    10
}
fn default_normalize_window_secs() -> f64 {
    5.0
}
fn default_max_fill_gain_db() -> f64 {
    12.0
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            min_gap_ms: default_min_gap_ms(),
            silence_peak_fraction: default_silence_peak_fraction(),
            scan_block_ms: default_scan_block_ms(),
            decode_chunk_secs: default_decode_chunk_secs(),
            silence_hold_ms: default_silence_hold_ms(),
            absolute_silence_rms: default_absolute_silence_rms(),
            scan_both: default_true(),
            gap_offset_tolerance_secs: default_gap_offset_tolerance_secs(),
            min_fill_correlation: default_min_fill_correlation(),
            fill_align_margin_secs: default_fill_align_margin_secs(),
            max_fill_align_adjustment_secs: default_max_fill_align_adjustment_secs(),
            fill_border_search_secs: default_fill_border_search_secs(),
            min_border_discovery_secs: default_min_border_discovery_secs(),
            border_standoff_secs: default_border_standoff_secs(),
            short_gap_mean_correlation_secs: default_short_gap_mean_correlation_secs(),
            fill_length_slack_secs: default_fill_length_slack_secs(),
            fill_seam_search_secs: default_fill_seam_search_secs(),
            gap_signature_context_secs: default_gap_signature_context_secs(),
            gap_signature_bin_ms: default_gap_signature_bin_ms(),
            min_structure_match_score: default_min_structure_match_score(),
            strong_structure_trust: default_strong_structure_trust(),
            disable_structure_trust: false,
            partial_structure_waveform_soften: default_partial_structure_waveform_soften(),
            crossfade_ms: default_crossfade_ms(),
            normalize_fill: default_true(),
            normalize_window_secs: default_normalize_window_secs(),
            max_fill_gain_db: default_max_fill_gain_db(),
            output: RepairOutputConfig::default(),
            dry_run: default_true(),
            limit_fill_to_mapped_region: default_true(),
        }
    }
}

fn default_video_codec() -> String {
    "copy".into()
}

fn default_audio_codec() -> String {
    "aac".into()
}

fn default_mux_audio_bitrate() -> String {
    "match_min".into()
}

/// Output configuration for the repair tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairOutputConfig {
    /// Write patched audio to this WAV file.
    pub wav_path: Option<PathBuf>,
    /// Mux patched audio into video A via ffmpeg (R5, requires `ffmpeg-mux` feature).
    pub video_path: Option<PathBuf>,
    /// Video stream codec for mux (`copy` preserves the source stream).
    #[serde(default = "default_video_codec")]
    pub video_codec: String,
    /// Audio stream codec for mux (patched WAV is encoded with this codec).
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,
    /// Mux AAC bitrate: `match_min`, `match_a`, `default`, or explicit e.g. `256k`.
    #[serde(default = "default_mux_audio_bitrate")]
    pub mux_audio_bitrate: String,
}

impl Default for RepairOutputConfig {
    fn default() -> Self {
        Self {
            wav_path: None,
            video_path: None,
            video_codec: default_video_codec(),
            audio_codec: default_audio_codec(),
            mux_audio_bitrate: default_mux_audio_bitrate(),
        }
    }
}

impl RepairConfig {
    pub fn scan_block_secs(&self) -> f64 {
        self.scan_block_ms as f64 / 1000.0
    }

    pub fn min_gap_secs(&self) -> f64 {
        self.min_gap_ms as f64 / 1000.0
    }

    pub fn silence_hold_blocks(&self) -> u32 {
        if self.scan_block_ms == 0 {
            return 0;
        }
        (self.silence_hold_ms as f64 / self.scan_block_ms as f64).ceil() as u32
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.min_gap_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "min_gap_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.silence_peak_fraction <= 0.0 || self.silence_peak_fraction >= 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "silence_peak_fraction".into(),
                reason: "must be between 0 and 1 exclusive".into(),
            });
        }
        if self.decode_chunk_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "decode_chunk_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.scan_block_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "scan_block_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.min_fill_correlation < -1.0 || self.min_fill_correlation > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_fill_correlation".into(),
                reason: "must be between -1.0 and 1.0 inclusive".into(),
            });
        }
        if self.max_fill_gain_db <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "max_fill_gain_db".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.normalize_window_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "normalize_window_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_offset_tolerance_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_offset_tolerance_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_align_margin_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_align_margin_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.max_fill_align_adjustment_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "max_fill_align_adjustment_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_border_search_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_border_search_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.min_border_discovery_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_border_discovery_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.border_standoff_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "border_standoff_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.short_gap_mean_correlation_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "short_gap_mean_correlation_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_length_slack_secs < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_length_slack_secs".into(),
                reason: "must be non-negative".into(),
            });
        }
        if self.fill_seam_search_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "fill_seam_search_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_signature_context_secs <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_signature_context_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.gap_signature_bin_ms == 0 {
            return Err(ConfigError::InvalidValue {
                field: "gap_signature_bin_ms".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if self.min_structure_match_score < 0.0 || self.min_structure_match_score > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "min_structure_match_score".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.strong_structure_trust < 0.0 || self.strong_structure_trust > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "strong_structure_trust".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.partial_structure_waveform_soften < 0.0
            || self.partial_structure_waveform_soften > 1.0
        {
            return Err(ConfigError::InvalidValue {
                field: "partial_structure_waveform_soften".into(),
                reason: "must be between 0.0 and 1.0 inclusive".into(),
            });
        }
        if self.partial_structure_waveform_soften > self.strong_structure_trust {
            return Err(ConfigError::InvalidValue {
                field: "partial_structure_waveform_soften".into(),
                reason: "must be less than or equal to strong_structure_trust".into(),
            });
        }
        if self.absolute_silence_rms < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "absolute_silence_rms".into(),
                reason: "must be non-negative".into(),
            });
        }
        #[cfg(not(feature = "ffmpeg-mux"))]
        if self.output.video_path.is_some() {
            return Err(ConfigError::InvalidValue {
                field: "repair.output.video_path".into(),
                reason: "requires building clip-sync-repair with --features ffmpeg-mux".into(),
            });
        }
        if parse_mux_audio_bitrate_policy(&self.output.mux_audio_bitrate).is_err() {
            return Err(ConfigError::InvalidValue {
                field: "repair.output.mux_audio_bitrate".into(),
                reason: "must be match_min, match_a, default, or a rate like 256k".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepairAppConfig {
    #[serde(flatten)]
    pub align: AlignConfig,
    #[serde(default)]
    pub repair: RepairConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Default for RepairAppConfig {
    fn default() -> Self {
        Self {
            align: default_repair_align_config(),
            repair: RepairConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

pub fn load_repair_app_config(path: Option<&Path>) -> Result<RepairAppConfig, AppError> {
    let Some(path) = path else {
        return Ok(RepairAppConfig::default());
    };

    let text = std::fs::read_to_string(path).map_err(|error| {
        AppError::Config(ConfigError::FileRead {
            path: path.to_path_buf(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    let num_clips_explicit = match toml::from_str::<toml::Table>(&text) {
        Ok(table) => table
            .get("clip")
            .and_then(toml::Value::as_table)
            .is_some_and(|clip| clip.contains_key("num_clips")),
        Err(_) => false,
    };

    let alignment_table = toml::from_str::<toml::Table>(&text)
        .ok()
        .and_then(|t| t.get("alignment").and_then(toml::Value::as_table).cloned())
        .unwrap_or_default();

    let require_consistent_explicit = alignment_table.contains_key("require_consistent_offsets");
    let high_rate_explicit = alignment_table.contains_key("refine_offset_high_rate");

    let mut config: RepairAppConfig = toml::from_str(&text).map_err(|error| {
        AppError::Config(ConfigError::Parse {
            detail: error.to_string(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    if !num_clips_explicit {
        config.align.clip.num_clips = REPAIR_DEFAULT_NUM_CLIPS;
    }
    if !require_consistent_explicit {
        config.align.alignment.require_consistent_offsets = false;
    }
    if !high_rate_explicit {
        config.align.alignment.refine_offset_high_rate = true;
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_app_config_defaults_to_two_clips() {
        let config = RepairAppConfig::default();
        assert_eq!(config.align.clip.num_clips, REPAIR_DEFAULT_NUM_CLIPS);
    }

    #[test]
    fn repair_app_config_allows_inconsistent_clip_offsets() {
        let config = RepairAppConfig::default();
        assert!(!config.align.alignment.require_consistent_offsets);
        assert!(config.align.alignment.prefer_start_clip);
    }

    #[test]
    fn load_config_applies_repair_num_clips_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert_eq!(config.align.clip.num_clips, REPAIR_DEFAULT_NUM_CLIPS);
    }

    #[test]
    fn load_config_respects_explicit_num_clips_one() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[clip]
num_clips = 1
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert_eq!(config.align.clip.num_clips, 1);
    }

    #[test]
    fn load_config_applies_repair_require_consistent_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[repair]
dry_run = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(!config.align.alignment.require_consistent_offsets);
    }

    #[test]
    fn repair_app_config_enables_high_rate_refinement_by_default() {
        let config = RepairAppConfig::default();
        assert!(config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn load_config_applies_high_rate_refinement_when_omitted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(&path, "[repair]\ndry_run = true\n").expect("write config");
        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn load_config_respects_explicit_high_rate_refinement_false() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(&path, "[alignment]\nrefine_offset_high_rate = false\n")
            .expect("write config");
        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(!config.align.alignment.refine_offset_high_rate);
    }

    #[test]
    fn rejects_min_gap_ms_zero() {
        let config = RepairConfig {
            min_gap_ms: 0,
            ..RepairConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_min_fill_correlation_out_of_range() {
        let config = RepairConfig {
            min_fill_correlation: 1.5,
            ..RepairConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn allows_min_fill_correlation_disable_gate() {
        let config = RepairConfig {
            min_fill_correlation: -1.0,
            ..RepairConfig::default()
        };
        config.validate().expect("disable gate should be valid");
    }

    #[test]
    fn load_config_respects_explicit_require_consistent_true() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("repair.toml");
        std::fs::write(
            &path,
            r#"
[alignment]
require_consistent_offsets = true
"#,
        )
        .expect("write config");

        let config = load_repair_app_config(Some(&path)).expect("load config");
        assert!(config.align.alignment.require_consistent_offsets);
    }
}
