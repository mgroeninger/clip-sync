use std::path::Path;

use serde::{Deserialize, Serialize};

use clip_sync::{unknown_toml_keys, AlignConfig, AppError, ConfigError, LoggingConfig};

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

    let config: AppConfig = toml::from_str(&raw).map_err(|error| {
        AppError::Config(ConfigError::Parse {
            detail: error.to_string(),
            source: Some(std::sync::Arc::new(error)),
        })
    })?;

    // `#[serde(flatten)]` on `align` rules out `deny_unknown_fields`, so serde
    // silently drops misspelled / unknown keys. Surface them so a typo in the
    // config does not read as "setting had no effect". Emitted with `eprintln!`
    // (not `tracing`) because tracing is not initialized until after the config
    // loads. Best-effort and non-fatal — a diagnostic must never fail the load.
    for key in unknown_toml_keys(&raw, &config) {
        eprintln!("warning: unknown config key `{key}` was ignored");
    }

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown_keys(raw: &str) -> Vec<String> {
        let config: AppConfig = toml::from_str(raw).expect("valid TOML for the test");
        unknown_toml_keys(raw, &config)
    }

    #[test]
    fn accepts_valid_config_without_flagging_known_keys() {
        let raw = r#"
            [clip]
            num_clips = 2

            [alignment]
            min_match_score = 0.3
            refine_offset_with_pcm = true

            [output]
            format = "json"

            [logging]
            level = "warn"
        "#;
        assert!(
            unknown_keys(raw).is_empty(),
            "no known key should be reported as unknown, got {:?}",
            unknown_keys(raw)
        );
    }

    #[test]
    fn flags_unknown_top_level_key() {
        // Every real setting lives under a table ([clip]/[alignment]/…), so any
        // bare top-level scalar key is unknown.
        let raw = "num_clips = 2\n";
        assert_eq!(unknown_keys(raw), vec!["num_clips".to_string()]);
    }

    #[test]
    fn flags_misspelled_key_inside_known_table() {
        let raw = "[alignment]\nmin_mtch_score = 0.3\n";
        assert_eq!(
            unknown_keys(raw),
            vec!["alignment.min_mtch_score".to_string()]
        );
    }

    #[test]
    fn flags_entirely_unknown_table() {
        let raw = "[bogus]\nkey = 1\n";
        assert_eq!(unknown_keys(raw), vec!["bogus".to_string()]);
    }
}
