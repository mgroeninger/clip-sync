use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use clip_sync::LogLevel;

use crate::infrastructure::config::OutputFormat;

#[derive(Debug, Parser)]
#[command(
    name = "clip-sync",
    about = "Align two videos by comparing audio fingerprints"
)]
pub struct Cli {
    /// Path to the first video file
    pub video_a: PathBuf,

    /// Path to the second video file
    pub video_b: PathBuf,

    /// Optional config file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Clip window length (e.g. 15m, 90s) [default: 15m]
    #[arg(long, value_parser = parse_duration)]
    pub clip_length: Option<std::time::Duration>,

    /// Number of clips to extract per video [default: 1]
    #[arg(long)]
    pub num_clips: Option<u32>,

    /// Output format [default: human]
    #[arg(long, value_enum)]
    pub format: Option<OutputFormatArg>,

    /// Show diagnostics and verbose progress
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,

    /// Log level for tracing [default: info]
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevelArg>,

    /// Write logs to a file
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Try every decodable audio track on both files and keep the best alignment.
    /// Use for multi-track MP4/MKV when the default track pick may be wrong (e.g. commentary
    /// at 48 kHz chosen over main program at 44.1 kHz). Slower: decodes each track pair.
    /// [default: disabled]
    #[arg(long, overrides_with = "no_try_all_tracks")]
    pub try_all_tracks: bool,

    /// Disable try-all-tracks (overrides config).
    #[arg(long, overrides_with = "try_all_tracks")]
    pub no_try_all_tracks: bool,

    /// After discovery alignment, FFT-refine offset on a short native-rate hold-out segment
    /// [default: disabled]
    #[arg(long, overrides_with = "no_refine_offset_high_rate")]
    pub refine_offset_high_rate: bool,

    /// Disable high-rate offset refinement (overrides config).
    #[arg(long, overrides_with = "refine_offset_high_rate")]
    pub no_refine_offset_high_rate: bool,

    /// When start/end Chromaprint offsets disagree, PCM-search the end clip around the
    /// high-confidence start offset [default: enabled].
    #[arg(long, overrides_with = "no_constrain_end_clip_to_start_offset")]
    pub constrain_end_clip_to_start_offset: bool,

    /// Keep independent end-clip Chromaprint when it disagrees with start (overrides config).
    #[arg(long, overrides_with = "constrain_end_clip_to_start_offset")]
    pub no_constrain_end_clip_to_start_offset: bool,

    /// After dual-anchor high-rate refinement, recompute recommended offset from updated
    /// clip offsets [default: enabled].
    #[arg(long, overrides_with = "no_high_rate_recommended_refusion")]
    pub high_rate_recommended_refusion: bool,

    /// Apply only the start-anchor high-rate tweak to recommended offset (legacy behavior).
    #[arg(long, overrides_with = "high_rate_recommended_refusion")]
    pub no_high_rate_recommended_refusion: bool,

    /// Check each clip for internal audio repetition (e.g. loops, rebroadcasts).
    /// Diagnostic only: never changes the exit code or recommended offset.
    #[arg(long)]
    pub check_clip_repetition: bool,

    /// After alignment, extract a hold-out window shifted by the recommended offset and score
    /// lag-0 similarity. Warns in human output when the hold-out confidence is below threshold.
    /// Diagnostic only: never changes the exit code or recommended offset.
    #[arg(long)]
    pub verify_offset: bool,

    /// Force query-reference alignment (short clip localized against long recording).
    #[arg(long)]
    pub query_reference: bool,

    /// Force symmetric multi-clip alignment (legacy behaviour).
    #[arg(long)]
    pub symmetric_align: bool,

    /// Coarse search stride on the reference timeline when using query-reference mode (seconds) [default: 60]
    #[arg(long, value_name = "SECS")]
    pub query_stride: Option<f64>,
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
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parse_duration_minutes() {
        assert_eq!(parse_duration("15m").unwrap().as_secs(), 15 * 60);
    }

    #[test]
    fn parse_duration_seconds() {
        assert_eq!(parse_duration("90s").unwrap().as_secs(), 90);
    }

    #[test]
    fn optional_overrides_remain_none_when_omitted() {
        let cli = Cli::parse_from(["clip-sync", "a.wav", "b.wav"]);
        assert!(cli.num_clips.is_none());
        assert!(cli.clip_length.is_none());
        assert!(cli.query_stride.is_none());
        assert!(cli.format.is_none());
        assert!(cli.log_level.is_none());
    }

    #[test]
    fn help_documents_align_defaults() {
        let help = Cli::command().render_help().to_string();

        for needle in [
            "[default: 15m]",
            "[default: 1]",
            "[default: human]",
            "[default: info]",
            "[default: 60]",
            "[default: disabled]",
        ] {
            assert!(help.contains(needle), "help missing {needle:?}:\n{help}");
        }
    }
}
