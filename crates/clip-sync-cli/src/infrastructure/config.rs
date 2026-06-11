use std::path::Path;

use serde::{Deserialize, Serialize};

use clip_sync::{AlignConfig, AppError, ConfigError, LoggingConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: OutputFormat,
    #[serde(default)]
    pub show_diagnostics: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(flatten)]
    pub align: AlignConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), AppError> {
        self.align.validate().map_err(AppError::Config)
    }
}

pub fn load_app_config(path: Option<&Path>) -> Result<AppConfig, AppError> {
    let Some(path) = path else {
        return Ok(AppConfig::default());
    };

    let raw = std::fs::read_to_string(path).map_err(|error| {
        AppError::Config(ConfigError::FileRead {
            path: path.to_path_buf(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    toml::from_str(&raw).map_err(|error| {
        AppError::Config(ConfigError::Parse {
            detail: error.to_string(),
            source: Some(std::sync::Arc::new(error)),
        })
    })
}
