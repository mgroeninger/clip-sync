use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::application::error::ConfigError;
use crate::domain::ClipPlan;

pub const DEFAULT_CLIP_LENGTH: Duration = Duration::from_secs(15 * 60);
pub const MIN_CLIP_LENGTH: Duration = Duration::from_secs(60);
pub const DEFAULT_NUM_CLIPS: u32 = 1;
pub const MIN_NUM_CLIPS: u32 = 1;
pub const DEFAULT_TARGET_SAMPLE_RATE: u32 = 11_025;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChromaprintPreset {
    #[default]
    Test2,
    Test3,
    Test4,
    Test5,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub clip: ClipConfig,
    #[serde(default)]
    pub alignment: AlignmentConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.clip.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipConfig {
    #[serde(with = "duration_secs", default = "default_clip_length")]
    pub clip_length: Duration,
    #[serde(default = "default_num_clips")]
    pub num_clips: u32,
    #[serde(default = "default_target_sample_rate")]
    pub target_sample_rate: Option<u32>,
    #[serde(default = "default_true")]
    pub normalize_loudness: bool,
    #[serde(default = "default_true")]
    pub trim_silence: bool,
    #[serde(default = "default_window_slide_secs")]
    pub window_slide_secs: u32,
    #[serde(default)]
    pub chromaprint_preset: ChromaprintPreset,
}

fn default_clip_length() -> Duration {
    DEFAULT_CLIP_LENGTH
}

fn default_num_clips() -> u32 {
    DEFAULT_NUM_CLIPS
}

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

fn default_target_sample_rate() -> Option<u32> {
    Some(DEFAULT_TARGET_SAMPLE_RATE)
}

fn default_true() -> bool {
    true
}

fn default_window_slide_secs() -> u32 {
    30
}

impl Default for ClipConfig {
    fn default() -> Self {
        Self {
            clip_length: DEFAULT_CLIP_LENGTH,
            num_clips: DEFAULT_NUM_CLIPS,
            target_sample_rate: default_target_sample_rate(),
            normalize_loudness: true,
            trim_silence: true,
            window_slide_secs: default_window_slide_secs(),
            chromaprint_preset: ChromaprintPreset::default(),
        }
    }
}

impl ClipConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.clip_length < MIN_CLIP_LENGTH {
            return Err(ConfigError::InvalidValue {
                field: "clip_length".into(),
                reason: format!("must be at least {} seconds", MIN_CLIP_LENGTH.as_secs()),
            });
        }
        if self.num_clips < MIN_NUM_CLIPS {
            return Err(ConfigError::InvalidValue {
                field: "num_clips".into(),
                reason: format!("must be at least {MIN_NUM_CLIPS}"),
            });
        }
        if let Some(rate) = self.target_sample_rate {
            if rate == 0 {
                return Err(ConfigError::InvalidValue {
                    field: "target_sample_rate".into(),
                    reason: "must be greater than 0".into(),
                });
            }
        }
        Ok(())
    }

    pub fn as_plan(&self) -> ClipPlan {
        ClipPlan::new(self.clip_length, self.num_clips)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentConfig {
    #[serde(default = "default_min_match_score")]
    pub min_match_score: f32,
    #[serde(default = "default_prefer_start_clip")]
    pub prefer_start_clip: bool,
    #[serde(default = "default_true")]
    pub require_consistent_offsets: bool,
    #[serde(default = "default_true")]
    pub refine_offset_with_pcm: bool,
    #[serde(default)]
    pub try_all_tracks: bool,
}

fn default_min_match_score() -> f32 {
    0.5
}

fn default_prefer_start_clip() -> bool {
    true
}

impl Default for AlignmentConfig {
    fn default() -> Self {
        Self {
            min_match_score: default_min_match_score(),
            prefer_start_clip: default_prefer_start_clip(),
            require_consistent_offsets: true,
            refine_offset_with_pcm: true,
            try_all_tracks: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: OutputFormat,
    #[serde(default)]
    pub show_diagnostics: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Human,
            show_diagnostics: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressMode {
    #[default]
    Auto,
    Quiet,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub level: LogLevel,
    pub log_file: Option<PathBuf>,
    #[serde(default)]
    pub progress: ProgressMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn default_num_clips_is_one() {
        assert_eq!(ClipConfig::default().num_clips, 1);
    }

    #[test]
    fn default_target_sample_rate_is_chromaprint_native() {
        assert_eq!(
            ClipConfig::default().target_sample_rate,
            Some(DEFAULT_TARGET_SAMPLE_RATE)
        );
    }

    #[test]
    fn rejects_clip_length_under_one_minute() {
        let config = ClipConfig {
            clip_length: Duration::from_secs(30),
            ..ClipConfig::default()
        };
        assert!(config.validate().is_err());
    }
}
