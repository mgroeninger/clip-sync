pub mod args;
pub mod exit_code;
pub mod output;

use std::process::ExitCode;

use clap::Parser;
use clip_sync::{init_tracing, StderrProgressReporter, SymphoniaMediaReader};

use clip_sync::AppError;

use crate::application::error::RepairError;
use crate::application::ports::GapReporter;
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::infrastructure::config::load_repair_app_config;

use self::args::Args;
use self::exit_code::exit_code_for;
use self::output::StdoutGapReporter;

pub fn run() -> ExitCode {
    let args = Args::parse();

    match run_inner(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::debug!(error = %e, "clip-sync-repair failed");
            eprintln!("error: {e}");
            exit_code_for(&e)
        }
    }
}

fn run_inner(args: Args) -> Result<(), RepairError> {
    let mut config = load_repair_app_config(args.config.as_deref())
        .map_err(RepairError::Align)?;

    // Apply CLI overrides.
    if let Some(ms) = args.min_gap_ms {
        config.repair.min_gap_ms = ms;
    }
    if let Some(f) = args.silence_fraction {
        config.repair.silence_peak_fraction = f;
    }
    if let Some(s) = args.scan_window_secs {
        config.repair.scan_window_secs = s;
    }
    if args.scan_both {
        config.repair.scan_both = true;
    } else if args.no_scan_both {
        config.repair.scan_both = false;
    }

    config.align.validate()
        .map_err(|e| RepairError::Align(AppError::Config(e)))?;
    config.repair.validate().map_err(|e| {
        RepairError::Config(e.to_string())
    })?;

    init_tracing(&config.logging).map_err(RepairError::Align)?;

    let progress = StderrProgressReporter::new(config.logging.progress);
    let media_reader = SymphoniaMediaReader;

    let request = ScanGapsRequest {
        video_a: args.video_a,
        video_b: args.video_b,
        align: config.align,
        scan_window_secs: config.repair.scan_window_secs,
        silence_peak_fraction: config.repair.silence_peak_fraction,
        min_gap_secs: config.repair.min_gap_secs(),
        scan_both: config.repair.scan_both,
        gap_offset_tolerance_secs: config.repair.gap_offset_tolerance_secs,
    };

    let report = ScanGaps::new(&media_reader, &progress).execute(request)?;

    let reporter = StdoutGapReporter { format: args.format };
    reporter.report(&report)?;

    Ok(())
}
