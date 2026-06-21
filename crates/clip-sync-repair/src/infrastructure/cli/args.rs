use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use clip_sync::LogLevel;

use crate::domain::{FillMode, FillOffsetMode};
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

    /// Clip window length (e.g. 15m, 90s) [default: 15m].
    #[arg(long, value_parser = parse_duration)]
    pub clip_length: Option<std::time::Duration>,

    /// Override: minimum gap duration to report (ms) [default: 1000].
    #[arg(long, value_name = "MS")]
    pub min_gap_ms: Option<u64>,

    /// Override: silence threshold as a fraction of peak amplitude [default: 0.01].
    #[arg(long, value_name = "FRACTION")]
    pub silence_fraction: Option<f32>,

    /// Override: decode chunk size for sequential scan (seconds) [default: 10].
    #[arg(long, value_name = "SECS", alias = "scan-window-secs")]
    pub decode_chunk_secs: Option<u64>,

    /// Override: analysis block size for silence detection (ms) [default: 250].
    #[arg(long, value_name = "MS")]
    pub scan_block_ms: Option<u64>,

    /// Override: consecutive non-silent time to absorb before closing a silence run (ms)
    /// [default: 500].
    #[arg(long, value_name = "MS")]
    pub silence_hold_ms: Option<u64>,

    /// Override: absolute RMS floor for silence detection (0–32767 scale; 0 disables)
    /// [default: 33].
    #[arg(long, value_name = "N")]
    pub absolute_silence_rms: Option<f32>,

    /// Enable bidirectional silence scan (scan B's timeline too) [default: enabled].
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

    /// Disable loudness normalization of fill segments [default: normalization enabled].
    #[arg(long)]
    pub no_normalize: bool,

    /// Stricter seam checks: always run the waveform Pearson gate (never skip it when
    /// structure scores are high), disable partial-structure threshold soften, and require
    /// both pre/post waveform seams to pass (no short-gap mean or one-strong-seam shortcuts).
    /// Does not disable structure matching on B or gap boundary extension retries.
    /// [fill-mode: gate only]
    #[arg(long)]
    pub no_structure_trust: bool,

    /// Override: minimum Pearson correlation at gap seams when the waveform gate runs
    /// [default: 0.35].
    #[arg(long, value_name = "N")]
    pub min_fill_correlation: Option<f32>,

    /// Override: maximum B fill slide during structure match (seconds) [default: 0.5].
    /// Under `fill_mode = fit`, structure search uses `fill_border_search_secs`; this key is
    /// reserved for future waveform-slide tuning (not the fine-polish loop).
    #[arg(long, value_name = "SECS")]
    pub max_fill_align_adjust_secs: Option<f64>,

    /// Unified fit scorer: structure term weight (`fill_mode = fit` only) [default: 0.35].
    #[arg(long, value_name = "N")]
    pub fill_fit_structure_weight: Option<f64>,

    /// Unified fit scorer: waveform term weight (`fill_mode = fit` only) [default: 0.65].
    #[arg(long, value_name = "N")]
    pub fill_fit_waveform_weight: Option<f64>,

    /// Override: A-side audio excluded adjacent to the dropout for border templates (seconds)
    /// [default: 0.35].
    #[arg(long, value_name = "SECS")]
    pub border_standoff_secs: Option<f64>,

    /// Per-gap B mapping: `recommended` (single offset) or `interpolated` (drift between clips).
    #[arg(long, value_enum, value_name = "MODE")]
    pub fill_offset: Option<FillOffsetMode>,

    /// Gap-fill placement: `gate` (legacy threshold checks) or `fit` (waveform slide search)
    /// [default: fit].
    #[arg(long, value_enum, value_name = "MODE")]
    pub fill_mode: Option<FillMode>,

    /// Disable post-seam gap-end extension retries when waveform correlation fails at the tail.
    #[arg(long)]
    pub no_gap_end_extend: bool,

    /// Disable pre-seam gap-start extension retries when waveform correlation fails at the head.
    #[arg(long)]
    pub no_gap_start_extend: bool,

    /// Disable short-gap fallback that patches when either seam passes (after mean rule fails).
    /// [fill-mode: gate only]
    #[arg(long)]
    pub no_short_gap_one_strong_seam: bool,

    /// Maximum gap-end extension when retrying a failed post seam (ms) [default: 500].
    #[arg(long, value_name = "MS")]
    pub gap_end_extend_max_ms: Option<u64>,

    /// Step size for gap-end extension retries (ms) [default: 20].
    #[arg(long, value_name = "MS")]
    pub gap_end_extend_step_ms: Option<u64>,

    /// Crossfade duration at gap boundaries (ms) [default: 10].
    #[arg(long, value_name = "MS")]
    pub crossfade_ms: Option<u64>,

    /// Override: number of alignment clips per video [default: 2].
    #[arg(long, value_name = "N")]
    pub num_clips: Option<u32>,

    /// Show diagnostics and verbose progress.
    #[arg(short, long)]
    pub verbose: bool,

    /// Suppress progress output.
    #[arg(short, long)]
    pub quiet: bool,

    /// Log level for tracing [default: info].
    #[arg(long, value_enum)]
    pub log_level: Option<LogLevelArg>,

    /// Write logs to a file (also logs to stderr).
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Try every decodable audio track on both files and keep the best alignment [default: disabled].
    #[arg(long, overrides_with = "no_try_all_tracks")]
    pub try_all_tracks: bool,

    /// Disable try-all-tracks (overrides config).
    #[arg(long, overrides_with = "try_all_tracks")]
    pub no_try_all_tracks: bool,

    /// After discovery alignment, FFT-refine offset on a short native-rate hold-out segment [default: enabled].
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

    /// Force query-reference alignment (short clip localized against long recording).
    #[arg(long)]
    pub query_reference: bool,

    /// Force symmetric multi-clip alignment (legacy behaviour).
    #[arg(long)]
    pub symmetric_align: bool,

    /// Coarse search stride on the reference timeline when using query-reference mode (seconds) [default: 60].
    #[arg(long, value_name = "SECS")]
    pub query_stride: Option<f64>,

    /// When using query-reference mode, allow filling gaps outside the located clip coverage
    /// [default: gaps outside mapped region are not fillable].
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
    use super::*;
    use clap::CommandFactory;
    use crate::infrastructure::config::RepairAppConfig;

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
        let args = Args::parse_from(["clip-sync-repair", "a.wav", "b.wav"]);
        assert!(args.min_gap_ms.is_none());
        assert!(args.clip_length.is_none());
        assert!(args.query_stride.is_none());
        assert!(args.log_level.is_none());
    }

    #[test]
    fn help_documents_repair_defaults() {
        let defaults = RepairAppConfig::default();
        let help = Args::command().render_help().to_string();

        for needle in [
            "[default: 15m]",
            &format!("[default: {}]", defaults.repair.min_gap_ms),
            &format!("[default: {}]", defaults.repair.silence_peak_fraction),
            &format!("[default: {}]", defaults.repair.decode_chunk_secs),
            &format!("[default: {}]", defaults.repair.scan_block_ms),
            &format!("[default: {}]", defaults.repair.silence_hold_ms),
            &format!("[default: {}]", defaults.repair.absolute_silence_rms as u32),
            &format!("[default: {}]", defaults.repair.min_fill_correlation),
            &format!("[default: {}]", defaults.repair.max_fill_align_adjustment_secs),
            &format!("[default: {}]", defaults.repair.fill_fit_structure_weight),
            &format!("[default: {}]", defaults.repair.fill_fit_waveform_weight),
            &format!("[default: {}]", defaults.repair.border_standoff_secs),
            &format!("[default: {}]", defaults.repair.crossfade_ms),
            "[default: fit]",
            "[fill-mode: gate only]",
            "[default: 2]",
            "[default: 60]",
            "[default: info]",
            "[default: enabled]",
            "[default: disabled]",
        ] {
            assert!(
                help.contains(needle),
                "help missing {needle:?}:\n{help}"
            );
        }
    }
}
