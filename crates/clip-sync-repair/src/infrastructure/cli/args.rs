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

    /// Override: scan window size (seconds).
    #[arg(long, value_name = "SECS")]
    pub scan_window_secs: Option<u64>,
}
