use tracing_subscriber::EnvFilter;

use crate::application::config::{LogLevel, LoggingConfig};
use crate::application::error::AppError;

pub mod progress;

pub use progress::StderrProgressReporter;

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
