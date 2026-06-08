use std::path::Path;

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

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            min_gap_ms: default_min_gap_ms(),
            silence_peak_fraction: default_silence_peak_fraction(),
            scan_window_secs: default_scan_window_secs(),
        }
    }
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
