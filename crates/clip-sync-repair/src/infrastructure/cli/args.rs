use std::path::PathBuf;

use clap::Parser;

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
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: OutputFormat,

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
}
