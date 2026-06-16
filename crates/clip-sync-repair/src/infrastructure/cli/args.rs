use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use clip_sync::LogLevel;

use crate::infrastructure::config::OutputFormat;

#[derive(Parser, Debug)]
#[command(
    name = "clip-sync-repair",
    about = "Scan two videos for audio gaps in A that B could fill",
    version
)]
pub struct Args {
    /// Video A — the file to scan for audio gaps.
    pub video_a: PathBuf,

    /// Video B — the reference file used to check if gaps are fillable.
    pub video_b: PathBuf,

    /// TOML config file (align + repair + logging sections).
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: OutputFormat,

    /// Clip window length (e.g. 15m, 90s).
    #[arg(long, value_parser = parse_duration)]
    pub clip_length: Option<std::time::Duration>,

    /// Override: minimum gap duration to report (ms).
    #[arg(long, value_name = "MS")]
    pub min_gap_ms: Option<u64>,

    /// Override: silence threshold as a fraction of peak amplitude.
    #[arg(long, value_name = "FRACTION")]
    pub silence_fraction: Option<f32>,

    /// Override: decode chunk size for sequential scan (seconds).
    #[arg(long, value_name = "SECS", alias = "scan-window-secs")]
    pub decode_chunk_secs: Option<u64>,

    /// Override: analysis block size for silence detection (ms).
    #[arg(long, value_name = "MS")]
    pub scan_block_ms: Option<u64>,

    /// Enable bidirectional silence scan (scan B's timeline too) — on by default.
    #[arg(long, overrides_with = "no_scan_both")]
    pub scan_both: bool,

    /// Disable bidirectional silence scan.
    #[arg(long, overrides_with = "scan_both")]
    pub no_scan_both: bool,

    /// Write patched audio to a WAV file (implies write mode).
    #[arg(long, value_name = "PATH")]
    pub wav: Option<PathBuf>,

    /// Mux patched audio into video A (implies write mode; requires `ffmpeg-mux` build feature).
    #[arg(long, value_name = "PATH")]
    pub mux: Option<PathBuf>,

    /// Disable loudness normalization of fill segments.
    #[arg(long)]
    pub no_normalize: bool,

    /// Crossfade duration at gap boundaries (ms).
    #[arg(long, value_name = "MS")]
    pub crossfade_ms: Option<u64>,

    /// Override: number of alignment clips per video (repair default: 2).
    #[arg(long, value_name = "N")]
    pub num_clips: Option<u32>,

    /// Show diagnostics and verbose progress.
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress progress output.
    #[arg(short, long)]
    pub quiet: bool,

    /// Log level for tracing.
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevelArg>,

    /// Write logs to a file (also logs to stderr).
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Try every decodable audio track on both files and keep the best alignment.
    #[arg(long, overrides_with = "no_try_all_tracks")]
    pub try_all_tracks: bool,

    /// Disable try-all-tracks (overrides config).
    #[arg(long, overrides_with = "try_all_tracks")]
    pub no_try_all_tracks: bool,

    /// After discovery alignment, FFT-refine offset on a short native-rate hold-out segment.
    #[arg(long, overrides_with = "no_refine_offset_high_rate")]
    pub refine_offset_high_rate: bool,

    /// Disable high-rate offset refinement (overrides config).
    #[arg(long, overrides_with = "refine_offset_high_rate")]
    pub no_refine_offset_high_rate: bool,

    /// Force query-reference alignment (short clip localized against long recording).
    #[arg(long)]
    pub query_reference: bool,

    /// Force symmetric multi-clip alignment (legacy behaviour).
    #[arg(long)]
    pub symmetric_align: bool,

    /// Coarse search stride on the reference timeline when using query-reference mode (seconds).
    #[arg(long, value_name = "SECS")]
    pub query_stride: Option<f64>,

    /// When using query-reference mode, allow filling gaps outside the located clip coverage.
    #[arg(long)]
    pub no_limit_fill_region: bool,
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
