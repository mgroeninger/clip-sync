pub mod args;
pub mod exit_code;
pub mod output;

use std::process::ExitCode;

use clap::Parser;
use clip_sync::{init_tracing, StderrProgressReporter, SymphoniaMediaReader};

use clip_sync::AppError;

use crate::application::error::RepairError;
use crate::application::patch_audio::{PatchAudio, PatchAudioRequest};
use crate::application::ports::{GapReporter, PatchedAudioWriter};
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::infrastructure::config::load_repair_app_config;
use crate::infrastructure::wav_writer::WavPatchedAudioWriter;

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
    if let Some(s) = args.decode_chunk_secs {
        config.repair.decode_chunk_secs = s;
    }
    if let Some(ms) = args.scan_block_ms {
        config.repair.scan_block_ms = ms;
    }
    if args.scan_both {
        config.repair.scan_both = true;
    } else if args.no_scan_both {
        config.repair.scan_both = false;
    }
    if let Some(wav_path) = args.wav {
        config.repair.output.wav_path = Some(wav_path);
        config.repair.dry_run = false;
    }
    if args.no_normalize {
        config.repair.normalize_fill = false;
    }
    if let Some(ms) = args.crossfade_ms {
        config.repair.crossfade_ms = ms;
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
        decode_chunk_secs: config.repair.decode_chunk_secs,
        scan_block_secs: config.repair.scan_block_secs(),
        silence_peak_fraction: config.repair.silence_peak_fraction,
        absolute_silence_rms: config.repair.absolute_silence_rms,
        silence_hold_blocks: config.repair.silence_hold_blocks(),
        min_gap_secs: config.repair.min_gap_secs(),
        scan_both: config.repair.scan_both,
        gap_offset_tolerance_secs: config.repair.gap_offset_tolerance_secs,
    };

    let report = ScanGaps::new(&media_reader, &progress).execute(request)?;

    // If not dry-run and a WAV output path is set, patch and write.
    // Capture the result rather than short-circuiting with `?` so the gap report is
    // always printed even when the write step fails.
    let write_result: Result<(), RepairError> = if !config.repair.dry_run {
        if let Some(ref wav_path) = config.repair.output.wav_path {
            let patch_request = PatchAudioRequest {
                report: report.clone(),
                normalize_fill: config.repair.normalize_fill,
                normalize_window_secs: config.repair.normalize_window_secs,
                max_fill_gain_db: config.repair.max_fill_gain_db,
                min_fill_correlation: config.repair.min_fill_correlation,
            };
            PatchAudio::new(&media_reader, &progress)
                .execute(patch_request, config.repair.crossfade_ms)
                .and_then(|patched| WavPatchedAudioWriter.write(&patched, wav_path))
        } else {
            Ok(())
        }
    } else {
        Ok(())
    };

    let reporter = StdoutGapReporter { format: args.format };
    reporter.report(&report)?;

    write_result
}
