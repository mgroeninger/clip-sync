use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::application::error::AppError;

pub mod progress;

pub use crate::application::extraction_progress::FINGERPRINT_ALIGN_STAGE;
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

/// Default `EnvFilter` when `RUST_LOG` is unset: app crates at `level`, symphonia demuxer noise at `error`, other third-party at `warn`.
pub fn default_env_filter(level: LogLevel) -> String {
    let app = log_level_name(level);
    format!("clip_sync={app},clip_sync_repair={app},symphonia_core=error,warn")
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

    use tracing_subscriber::fmt::format::FmtSpan;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_env_filter(config.level)));

    // Perf instrumentation (Level A, archive/TEMP-production-repair-perf-plan.md §0): when
    // `CLIP_SYNC_SPAN_TIMING` is set, emit a line on every span close carrying `time.busy` / `time.idle`.
    // `scripts/measure-repair-perf.ps1` reads these back, keyed by each span's FULL parent chain (the chain
    // is on the close line, so nesting is recoverable), and reports a span tree with exclusive costs. Note
    // a parent's `time.busy` INCLUDES its children's. Off by default — no change to normal output. Combine
    // with e.g. `RUST_LOG=clip_sync_repair=info` and a `--release` build.
    let span_events = if std::env::var_os("CLIP_SYNC_SPAN_TIMING").is_some() {
        FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

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
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(span_events.clone())
                    .with_writer(std::io::stderr),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(span_events)
                    .with_writer(file)
                    .with_ansi(false),
            )
            .try_init()
            .ok();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(span_events)
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
            "clip_sync=info,clip_sync_repair=info,symphonia_core=error,warn"
        );
        assert_eq!(
            default_env_filter(LogLevel::Debug),
            "clip_sync=debug,clip_sync_repair=debug,symphonia_core=error,warn"
        );
    }
}
