use std::path::Path;

use crate::application::config::AlignConfig;
use crate::application::error::{AppError, ConfigError};

pub fn load_align_config(path: Option<&Path>) -> Result<AlignConfig, AppError> {
    let Some(path) = path else {
        return Ok(AlignConfig::default());
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
