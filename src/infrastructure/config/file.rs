use std::path::Path;

use crate::application::config::AppConfig;
use crate::application::error::{AppError, ConfigError};

pub fn load_optional_config_file(path: Option<&Path>) -> Result<AppConfig, AppError> {
    let Some(path) = path else {
        return Ok(AppConfig::default());
    };

    let raw = std::fs::read_to_string(path)
        .map_err(|_| AppError::Config(ConfigError::FileRead(path.to_path_buf())))?;

    toml::from_str(&raw).map_err(|error| AppError::Config(ConfigError::Parse(error.to_string())))
}
