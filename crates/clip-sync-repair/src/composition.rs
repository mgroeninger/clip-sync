//! Binary composition root: default adapter wiring and config → use-case mapping.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clip_sync::{ProgressReporter, SymphoniaMediaReader};

use crate::application::error::RepairError;
use crate::application::run_repair::{PendingRepairWrite, RepairRunInput, RepairRunOutcome, run_repair};
use crate::application::scan_gaps::ScanGapsRequest;
use crate::infrastructure::aligner::SymphoniaAligner;
use crate::infrastructure::cli::{self, args::Args, exit_code::exit_code_for, output::print_repair_output};
use crate::infrastructure::config::RepairAppConfig;
use crate::infrastructure::wav_writer::WavPatchedAudioWriter;

#[cfg(feature = "ffmpeg-mux")]
use crate::application::mux_bitrate::parse_mux_audio_bitrate_policy;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::MuxOptions;
#[cfg(feature = "ffmpeg-mux")]
use crate::infrastructure::ffmpeg_mux::FfmpegMediaMuxer;

pub fn run() -> ExitCode {
    match run_inner(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::debug!(error = %e, "clip-sync-repair failed");
            eprintln!("error: {e}");
            exit_code_for(&e)
        }
    }
}

fn run_inner(args: Args) -> Result<(), RepairError> {
    #[cfg(not(feature = "ffmpeg-mux"))]
    if args.mux.is_some() {
        return Err(RepairError::Config(
            "--mux requires clip-sync-repair built with --features ffmpeg-mux".into(),
        ));
    }

    let mut config = crate::infrastructure::config::load_repair_app_config(args.config.as_deref())
        .map_err(RepairError::Align)?;
    cli::apply_cli_overrides(&mut config, &args);
    validate_config(&config)?;

    clip_sync::init_tracing(&config.logging).map_err(RepairError::Align)?;

    let progress = clip_sync::StderrProgressReporter::new(config.logging.progress);
    progress.phase(&format!(
        "clip-sync-repair: aligning {} with {}",
        args.video_a.display(),
        args.video_b.display()
    ));

    let input = repair_run_input(
        &config,
        args.video_a.clone(),
        args.video_b.clone(),
    )?;
    let outcome = run_repair_with_defaults(input, &progress)?;

    print_repair_outcome(&args, &config, outcome)
}

fn validate_config(config: &RepairAppConfig) -> Result<(), RepairError> {
    config
        .align
        .validate()
        .map_err(|e| RepairError::Align(clip_sync::AppError::Config(e)))?;
    config
        .repair
        .validate()
        .map_err(|e| RepairError::Config(e.to_string()))
}

pub fn repair_run_input(
    config: &RepairAppConfig,
    video_a: PathBuf,
    video_b: PathBuf,
) -> Result<RepairRunInput, RepairError> {
    Ok(RepairRunInput {
        scan: ScanGapsRequest {
            video_a: video_a.clone(),
            video_b,
            align: config.align.clone(),
            decode_chunk_secs: config.repair.decode_chunk_secs,
            scan_block_secs: config.repair.scan_block_secs(),
            silence_peak_fraction: config.repair.silence_peak_fraction,
            absolute_silence_rms: config.repair.absolute_silence_rms,
            silence_hold_blocks: config.repair.silence_hold_blocks(),
            min_gap_secs: config.repair.min_gap_secs(),
            scan_both: config.repair.scan_both,
            gap_offset_tolerance_secs: config.repair.gap_offset_tolerance_secs,
            limit_fill_to_mapped_region: config.repair.limit_fill_to_mapped_region,
        },
        write: pending_repair_write(config, video_a)?,
    })
}

fn pending_repair_write(
    config: &RepairAppConfig,
    source_video: PathBuf,
) -> Result<Option<PendingRepairWrite>, RepairError> {
    if config.repair.dry_run {
        return Ok(None);
    }

    let wants_wav = config.repair.output.wav_path.is_some();
    #[cfg(feature = "ffmpeg-mux")]
    let wants_mux = config.repair.output.video_path.is_some();
    #[cfg(not(feature = "ffmpeg-mux"))]
    let wants_mux = false;

    if !wants_wav && !wants_mux {
        return Ok(None);
    }

    Ok(Some(PendingRepairWrite {
        source_video,
        patch_settings: config.repair.patch_settings(),
        crossfade_ms: config.repair.crossfade_ms,
        wav_path: config.repair.output.wav_path.clone(),
        #[cfg(feature = "ffmpeg-mux")]
        video_path: config.repair.output.video_path.clone(),
        #[cfg(feature = "ffmpeg-mux")]
        mux_options: MuxOptions {
            video_codec: config.repair.output.video_codec.clone(),
            audio_codec: config.repair.output.audio_codec.clone(),
            audio_bitrate: None,
        },
        #[cfg(feature = "ffmpeg-mux")]
        mux_audio_bitrate_policy: parse_mux_audio_bitrate_policy(
            &config.repair.output.mux_audio_bitrate,
        )
        .map_err(RepairError::Config)?,
    }))
}

#[cfg(feature = "ffmpeg-mux")]
fn run_repair_with_defaults(
    input: RepairRunInput,
    progress: &dyn ProgressReporter,
) -> Result<RepairRunOutcome, RepairError> {
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let wav_writer = WavPatchedAudioWriter;
    let muxer = FfmpegMediaMuxer;
    run_repair(
        input,
        &media_reader,
        &aligner,
        &wav_writer,
        &muxer,
        progress,
    )
}

#[cfg(not(feature = "ffmpeg-mux"))]
fn run_repair_with_defaults(
    input: RepairRunInput,
    progress: &dyn ProgressReporter,
) -> Result<RepairRunOutcome, RepairError> {
    let media_reader = SymphoniaMediaReader;
    let aligner = SymphoniaAligner;
    let wav_writer = WavPatchedAudioWriter;
    run_repair(input, &media_reader, &aligner, &wav_writer, progress)
}

fn print_repair_outcome(
    args: &Args,
    config: &RepairAppConfig,
    outcome: RepairRunOutcome,
) -> Result<(), RepairError> {
    let patch_summary = outcome
        .patch_result
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref().map(|patch| &patch.summary));

    let output_written = outcome
        .patch_result
        .as_ref()
        .ok()
        .and_then(|result| result.as_ref())
        .filter(|result| result.summary.has_patches())
        .and({
            #[cfg(feature = "ffmpeg-mux")]
            {
                config
                    .repair
                    .output
                    .video_path
                    .as_deref()
                    .or(config.repair.output.wav_path.as_deref())
            }
            #[cfg(not(feature = "ffmpeg-mux"))]
            {
                config.repair.output.wav_path.as_deref()
            }
        });

    print_repair_output(
        &outcome.report,
        patch_summary,
        outcome
            .patch_result
            .as_ref()
            .ok()
            .and_then(|result| result.as_ref()),
        args.format,
        args.verbose,
        output_written,
    )?;

    outcome.patch_result.map(|_| ())
}
