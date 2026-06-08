use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use clip_sync::{AlignConfig, AppError, ConfigError, LoggingConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairConfig {
    /// Minimum silent window duration (ms) to include in the gap report.
    #[serde(default = "default_min_gap_ms")]
    pub min_gap_ms: u64,
    /// Fraction of peak amplitude below which a block is considered silent.
    #[serde(default = "default_silence_peak_fraction")]
    pub silence_peak_fraction: f32,
    /// Duration of each scan window when checking A's timeline for silence (seconds).
    #[serde(default = "default_scan_window_secs")]
    pub scan_window_secs: u64,
    /// Also scan B's native timeline for silence to produce `gap_offset_agreement`.
    #[serde(default = "default_true")]
    pub scan_both: bool,
    /// Maximum |silence_offset − alignment_offset| (seconds) to count as agreement.
    #[serde(default = "default_gap_offset_tolerance_secs")]
    pub gap_offset_tolerance_secs: f64,
    /// Minimum normalized Pearson correlation between A's pre-gap border audio and the
    /// start of B's fill segment. Regions below this threshold are skipped during patch.
    #[serde(default = "default_min_fill_correlation")]
    pub min_fill_correlation: f32,
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
}

fn default_min_gap_ms() -> u64 {
    100
}
fn default_silence_peak_fraction() -> f32 {
    0.01
}
fn default_scan_window_secs() -> u64 {
    60
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
            scan_window_secs: default_scan_window_secs(),
            scan_both: default_true(),
            gap_offset_tolerance_secs: default_gap_offset_tolerance_secs(),
            min_fill_correlation: default_min_fill_correlation(),
            crossfade_ms: default_crossfade_ms(),
            normalize_fill: default_true(),
            normalize_window_secs: default_normalize_window_secs(),
            max_fill_gain_db: default_max_fill_gain_db(),
            output: RepairOutputConfig::default(),
            dry_run: default_true(),
        }
    }
}

/// Output configuration for the repair tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairOutputConfig {
    /// Write patched audio to this WAV file.
    pub wav_path: Option<PathBuf>,
}

impl RepairConfig {
    pub fn min_gap_secs(&self) -> f64 {
        self.min_gap_ms as f64 / 1000.0
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.silence_peak_fraction <= 0.0 || self.silence_peak_fraction >= 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "silence_peak_fraction".into(),
                reason: "must be between 0 and 1 exclusive".into(),
            });
        }
        if self.scan_window_secs == 0 {
            return Err(ConfigError::InvalidValue {
                field: "scan_window_secs".into(),
                reason: "must be greater than zero".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairAppConfig {
    #[serde(flatten)]
    pub align: AlignConfig,
    #[serde(default)]
    pub repair: RepairConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

pub fn load_repair_app_config(path: Option<&Path>) -> Result<RepairAppConfig, AppError> {
    let Some(path) = path else {
        return Ok(RepairAppConfig::default());
    };

    let text = std::fs::read_to_string(path)
        .map_err(|_| AppError::Config(ConfigError::FileRead(path.to_path_buf())))?;

    toml::from_str::<RepairAppConfig>(&text)
        .map_err(|e| AppError::Config(ConfigError::Parse(e.to_string())))
}
