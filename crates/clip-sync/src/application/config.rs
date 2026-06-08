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
pub struct AlignConfig {
    #[serde(default)]
    pub clip: ClipConfig,
    #[serde(default)]
    pub alignment: AlignmentConfig,
}

impl AlignConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.clip.validate()
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
    /// Extra seconds extracted either side of window for subclip sliding (0 = disabled).
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
    /// After discovery alignment, re-extract a short native-rate hold-out and FFT-refine Δ.
    #[serde(default)]
    pub refine_offset_high_rate: bool,
    /// Hold-out segment length for high-rate refinement (seconds).
    #[serde(default = "default_high_rate_refine_secs")]
    pub high_rate_refine_secs: u32,
    /// Maximum |adjustment| applied from high-rate refinement.
    #[serde(default = "default_high_rate_refine_max_adjustment_secs")]
    pub high_rate_refine_max_adjustment_secs: f64,
    /// End clip: refine around the start-clip offset instead of independent Chromaprint discovery.
    #[serde(default = "default_true")]
    pub refine_end_clip_around_start_offset: bool,
    /// PCM search radius (seconds) when refining the end clip around the start offset.
    #[serde(default = "default_end_clip_refine_radius_secs")]
    pub end_clip_refine_radius_secs: f64,
    /// Place the end alignment window at decodable audio extent, not container duration.
    #[serde(default = "default_true")]
    pub clamp_end_clip_to_decodable_extent: bool,
    /// Inset (seconds) before decodable extent when anchoring the end clip window.
    #[serde(default = "default_end_clip_tail_inset_secs")]
    pub end_clip_tail_inset_secs: f64,
    /// Skip end-clip alignment when tail extract decoded too little audio or had decode skips.
    #[serde(default = "default_true")]
    pub skip_unreliable_end_clip: bool,
    /// Minimum decoded fraction of the end window required to align the end clip.
    #[serde(default = "default_min_end_clip_decode_fraction")]
    pub min_end_clip_decode_fraction: f64,
    /// End clip is unreliable when decode skips exceed this count.
    #[serde(default = "default_max_end_clip_decode_skips")]
    pub max_end_clip_decode_skips: u32,
}

fn default_end_clip_tail_inset_secs() -> f64 {
    1.0
}

fn default_min_end_clip_decode_fraction() -> f64 {
    0.95
}

fn default_max_end_clip_decode_skips() -> u32 {
    8
}

fn default_end_clip_refine_radius_secs() -> f64 {
    5.0
}

fn default_high_rate_refine_secs() -> u32 {
    3
}

fn default_high_rate_refine_max_adjustment_secs() -> f64 {
    0.1
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
            refine_offset_high_rate: false,
            high_rate_refine_secs: default_high_rate_refine_secs(),
            high_rate_refine_max_adjustment_secs: default_high_rate_refine_max_adjustment_secs(),
            refine_end_clip_around_start_offset: true,
            end_clip_refine_radius_secs: default_end_clip_refine_radius_secs(),
            clamp_end_clip_to_decodable_extent: true,
            end_clip_tail_inset_secs: default_end_clip_tail_inset_secs(),
            skip_unreliable_end_clip: true,
            min_end_clip_decode_fraction: default_min_end_clip_decode_fraction(),
            max_end_clip_decode_skips: default_max_end_clip_decode_skips(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        AlignConfig::default().validate().unwrap();
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
