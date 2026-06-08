use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use crate::application::error::AppError;

pub mod progress;

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

pub fn init_tracing(config: &LoggingConfig) -> Result<(), AppError> {
    let level = match config.level {
        LogLevel::Error => "error",
        LogLevel::Warn => "warn",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .ok();

    if config.log_file.is_some() {
        tracing::warn!("log file output is not yet implemented");
    }

    Ok(())
}
