pub mod args;
pub mod exit_code;
pub mod output;

use std::process::ExitCode;

use clap::Parser;
use clip_sync::{init_tracing, StderrProgressReporter, SymphoniaMediaReader};

use clip_sync::AppError;

use crate::application::error::RepairError;
use crate::application::patch_audio::PatchAudioRequest;
use crate::application::ports::GapReporter;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::MuxOptions;
use crate::application::repair_videos::{RepairVideos, RepairWriteRequest};
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::infrastructure::config::load_repair_app_config;
use crate::infrastructure::wav_writer::WavPatchedAudioWriter;

#[cfg(feature = "ffmpeg-mux")]
use crate::infrastructure::ffmpeg_mux::FfmpegMediaMuxer;

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
    #[cfg(not(feature = "ffmpeg-mux"))]
    if args.mux.is_some() {
        return Err(RepairError::Config(
            "--mux requires clip-sync-repair built with --features ffmpeg-mux".into(),
        ));
    }

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
    #[cfg(feature = "ffmpeg-mux")]
    if let Some(mux_path) = args.mux {
        config.repair.output.video_path = Some(mux_path);
        config.repair.dry_run = false;
    }
    if args.no_normalize {
        config.repair.normalize_fill = false;
    }
    if let Some(ms) = args.crossfade_ms {
        config.repair.crossfade_ms = ms;
    }
    if let Some(num_clips) = args.num_clips {
        config.align.clip.num_clips = num_clips;
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
        video_a: args.video_a.clone(),
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

    // Patch/write when not dry-run and an output path is set.
    // Capture the result rather than short-circuiting with `?` so the gap report is
    // always printed even when the write step fails.
    let write_result: Result<(), RepairError> = if !config.repair.dry_run {
        let wants_wav = config.repair.output.wav_path.is_some();
        #[cfg(feature = "ffmpeg-mux")]
        let wants_mux = config.repair.output.video_path.is_some();
        #[cfg(not(feature = "ffmpeg-mux"))]
        let wants_mux = false;

        if wants_wav || wants_mux {
            let patch_request = PatchAudioRequest {
                report: report.clone(),
                normalize_fill: config.repair.normalize_fill,
                normalize_window_secs: config.repair.normalize_window_secs,
                max_fill_gain_db: config.repair.max_fill_gain_db,
                min_fill_correlation: config.repair.min_fill_correlation,
                fill_align_margin_secs: config.repair.fill_align_margin_secs,
                max_fill_align_adjustment_secs: config.repair.max_fill_align_adjustment_secs,
            };

            let write_request = RepairWriteRequest {
                source_video: args.video_a,
                patch_request,
                crossfade_ms: config.repair.crossfade_ms,
                wav_path: config.repair.output.wav_path.clone(),
                #[cfg(feature = "ffmpeg-mux")]
                video_path: config.repair.output.video_path.clone(),
                #[cfg(feature = "ffmpeg-mux")]
                mux_options: MuxOptions {
                    video_codec: config.repair.output.video_codec.clone(),
                    audio_codec: config.repair.output.audio_codec.clone(),
                },
            };

            let repair = RepairVideos::new(&media_reader, &progress, &WavPatchedAudioWriter);
            #[cfg(feature = "ffmpeg-mux")]
            {
                let muxer = FfmpegMediaMuxer;
                repair.execute(write_request, &muxer)
            }
            #[cfg(not(feature = "ffmpeg-mux"))]
            {
                repair.execute(write_request)
            }
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
