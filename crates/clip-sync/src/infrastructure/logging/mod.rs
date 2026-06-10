use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::application::error::AppError;

pub mod extraction_progress;
pub mod progress;

pub use extraction_progress::{ExtractionProgressScope, FINGERPRINT_ALIGN_STAGE};
pub use progress::StderrProgressReporter;

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

/// Default `EnvFilter` when `RUST_LOG` is unset: clip-sync crates at `level`, all other targets at `warn`.
pub fn default_env_filter(level: LogLevel) -> String {
    let app = log_level_name(level);
    format!("clip_sync={app},clip_sync_repair={app},warn")
}

fn log_level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    }
}

#[cfg(feature = "default-tracing")]
pub fn init_tracing(config: &LoggingConfig) -> Result<(), AppError> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    use crate::application::error::ConfigError;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_env_filter(config.level)));

    if let Some(path) = &config.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                AppError::Config(ConfigError::InvalidValue {
                    field: "log_file".into(),
                    reason: format!("failed to open: {error}"),
                })
            })?;

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(file)
                    .with_ansi(false),
            )
            .try_init()
            .ok();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
            .ok();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_env_filter_sets_app_crates_and_warn_root() {
        assert_eq!(
            default_env_filter(LogLevel::Info),
            "clip_sync=info,clip_sync_repair=info,warn"
        );
        assert_eq!(
            default_env_filter(LogLevel::Debug),
            "clip_sync=debug,clip_sync_repair=debug,warn"
        );
    }
}
