use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::application::config::{LogLevel, OutputFormat};

#[derive(Debug, Parser)]
#[command(name = "clip-sync", about = "Align two videos by comparing audio fingerprints")]
pub struct Cli {
    /// Path to the first video file
    pub video_a: PathBuf,

    /// Path to the second video file
    pub video_b: PathBuf,

    /// Optional config file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Clip window length (e.g. 15m, 90s)
    #[arg(long, value_parser = parse_duration)]
    pub clip_length: Option<std::time::Duration>,

    /// Number of clips to extract per video
    #[arg(long)]
    pub num_clips: Option<u32>,

    /// Output format
    #[arg(long, value_enum)]
    pub format: Option<OutputFormatArg>,

    /// Show diagnostics and verbose progress
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,

    /// Log level for tracing
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevelArg>,

    /// Write logs to a file
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Try every decodable audio track on both files and keep the best alignment.
    /// Use for multi-track MP4/MKV when the default track pick may be wrong (e.g. commentary
    /// at 48 kHz chosen over main program at 44.1 kHz). Slower: decodes each track pair.
    #[arg(long)]
    pub try_all_tracks: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormatArg {
    Human,
    Json,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(value: OutputFormatArg) -> Self {
        match value {
            OutputFormatArg::Human => OutputFormat::Human,
            OutputFormatArg::Json => OutputFormat::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevelArg {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevelArg> for LogLevel {
    fn from(value: LogLevelArg) -> Self {
        match value {
            LogLevelArg::Error => LogLevel::Error,
            LogLevelArg::Warn => LogLevel::Warn,
            LogLevelArg::Info => LogLevel::Info,
            LogLevelArg::Debug => LogLevel::Debug,
            LogLevelArg::Trace => LogLevel::Trace,
        }
    }
}

fn parse_duration(raw: &str) -> Result<std::time::Duration, String> {
    let raw = raw.trim().to_ascii_lowercase();
    if let Some(stripped) = raw.strip_suffix('m') {
        let minutes: u64 = stripped
            .parse()
            .map_err(|_| format!("invalid duration: {raw}"))?;
        return Ok(std::time::Duration::from_secs(minutes * 60));
    }
    if let Some(stripped) = raw.strip_suffix('s') {
        let seconds: u64 = stripped
            .parse()
            .map_err(|_| format!("invalid duration: {raw}"))?;
        return Ok(std::time::Duration::from_secs(seconds));
    }
    let seconds: u64 = raw
        .parse()
        .map_err(|_| format!("invalid duration: {raw}"))?;
    Ok(std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(parse_duration("15m").unwrap().as_secs(), 15 * 60);
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(parse_duration("90s").unwrap().as_secs(), 90);
    }
}
