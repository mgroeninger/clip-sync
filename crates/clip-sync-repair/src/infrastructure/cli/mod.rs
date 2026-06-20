pub mod args;
pub mod exit_code;
pub mod output;

use std::process::ExitCode;

use clap::Parser;
use clip_sync::{
    init_tracing, AlignmentMode, ProgressMode, ProgressReporter, StderrProgressReporter,
    SymphoniaMediaReader,
};

use clip_sync::AppError;

use crate::application::error::RepairError;
use crate::application::patch_audio::PatchAudioRequest;
#[cfg(feature = "ffmpeg-mux")]
use crate::application::ports::MuxOptions;
use crate::application::repair_videos::{RepairVideos, RepairWriteRequest};
use crate::application::scan_gaps::{ScanGaps, ScanGapsRequest};
use crate::infrastructure::aligner::SymphoniaAligner;
use crate::infrastructure::config::{load_repair_app_config, RepairAppConfig};
use crate::infrastructure::wav_writer::WavPatchedAudioWriter;

#[cfg(feature = "ffmpeg-mux")]
use crate::infrastructure::ffmpeg_mux::FfmpegMediaMuxer;
#[cfg(feature = "ffmpeg-mux")]
use crate::infrastructure::mux_bitrate::parse_mux_audio_bitrate_policy;

use self::args::Args;
use self::exit_code::exit_code_for;
use self::output::print_repair_output;

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

    apply_cli_overrides(&mut config, &args);

    config.align.validate()
        .map_err(|e| RepairError::Align(AppError::Config(e)))?;
    config.repair.validate().map_err(|e| {
        RepairError::Config(e.to_string())
    })?;

    init_tracing(&config.logging).map_err(RepairError::Align)?;

    let progress = StderrProgressReporter::new(config.logging.progress);
    progress.phase(&format!(
        "clip-sync-repair: aligning {} with {}",
        args.video_a.display(),
        args.video_b.display()
    ));
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
        limit_fill_to_mapped_region: config.repair.limit_fill_to_mapped_region,
    };

    let aligner = SymphoniaAligner;
    let report = ScanGaps::new(&media_reader, &progress, &aligner).execute(request)?;

    // Patch/write when not dry-run and an output path is set.
    // Capture the result rather than short-circuiting with `?` so the gap report is
    // always printed even when the write step fails.
    let write_result: Result<Option<crate::application::PatchAudioResult>, RepairError> =
        if !config.repair.dry_run {
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
                    fill_border_search_secs: config.repair.fill_border_search_secs,
                    min_border_discovery_secs: config.repair.min_border_discovery_secs,
                    border_standoff_secs: config.repair.border_standoff_secs,
                    short_gap_mean_correlation_secs: config.repair.short_gap_mean_correlation_secs,
                    fill_length_slack_secs: config.repair.fill_length_slack_secs,
                    fill_seam_search_secs: config.repair.fill_seam_search_secs,
                    gap_signature_context_secs: config.repair.gap_signature_context_secs,
                    gap_signature_bin_ms: config.repair.gap_signature_bin_ms,
                    min_structure_match_score: config.repair.min_structure_match_score,
                    strong_structure_trust: config.repair.strong_structure_trust,
                    disable_structure_trust: config.repair.disable_structure_trust,
                    partial_structure_waveform_soften: config
                        .repair
                        .partial_structure_waveform_soften,
                    absolute_silence_rms: config.repair.absolute_silence_rms,
                    fill_offset_mode: config.repair.fill_offset_mode,
                    gap_end_extend_on_post_seam_fail: config
                        .repair
                        .gap_end_extend_on_post_seam_fail,
                    gap_start_extend_on_pre_seam_fail: config
                        .repair
                        .gap_start_extend_on_pre_seam_fail,
                    gap_end_extend_max_ms: config.repair.gap_end_extend_max_ms,
                    gap_end_extend_step_ms: config.repair.gap_end_extend_step_ms,
                    short_gap_one_strong_seam_fallback: config
                        .repair
                        .short_gap_one_strong_seam_fallback,
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
                        audio_bitrate: None,
                    },
                    #[cfg(feature = "ffmpeg-mux")]
                    mux_audio_bitrate_policy: parse_mux_audio_bitrate_policy(
                        &config.repair.output.mux_audio_bitrate,
                    )
                    .map_err(|reason| RepairError::Config(reason))?,
                };

                let repair = RepairVideos::new(&media_reader, &progress, &WavPatchedAudioWriter);
                #[cfg(feature = "ffmpeg-mux")]
                {
                    let muxer = FfmpegMediaMuxer;
                    repair
                        .execute(write_request, &muxer)
                        .map(Some)
                }
                #[cfg(not(feature = "ffmpeg-mux"))]
                {
                    repair.execute(write_request).map(Some)
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        };

    let patch_summary = write_result
        .as_ref()
        .ok()
        .and_then(|r| r.as_ref().map(|result| &result.summary));

    let output_written = write_result
        .as_ref()
        .ok()
        .and_then(|r| r.as_ref())
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
        &report,
        patch_summary,
        write_result.as_ref().ok().and_then(|r| r.as_ref()),
        args.format,
        args.verbose,
        output_written,
    )?;

    write_result.map(|_| ())
}

fn apply_cli_overrides(config: &mut RepairAppConfig, args: &Args) {
    if let Some(duration) = args.clip_length {
        config.align.clip.clip_length = duration;
    }
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
    if let Some(ms) = args.silence_hold_ms {
        config.repair.silence_hold_ms = ms;
    }
    if let Some(rms) = args.absolute_silence_rms {
        config.repair.absolute_silence_rms = rms;
    }
    if args.scan_both {
        config.repair.scan_both = true;
    } else if args.no_scan_both {
        config.repair.scan_both = false;
    }
    if let Some(wav_path) = &args.wav {
        config.repair.output.wav_path = Some(wav_path.clone());
        config.repair.dry_run = false;
    }
    #[cfg(feature = "ffmpeg-mux")]
    if let Some(mux_path) = &args.mux {
        config.repair.output.video_path = Some(mux_path.clone());
        config.repair.dry_run = false;
    }
    if args.no_normalize {
        config.repair.normalize_fill = false;
    }
    if args.no_structure_trust {
        config.repair.disable_structure_trust = true;
    }
    if let Some(corr) = args.min_fill_correlation {
        config.repair.min_fill_correlation = corr;
    }
    if let Some(secs) = args.max_fill_align_adjust_secs {
        config.repair.max_fill_align_adjustment_secs = secs;
    }
    if let Some(secs) = args.border_standoff_secs {
        config.repair.border_standoff_secs = secs;
    }
    if let Some(mode) = args.fill_offset {
        config.repair.fill_offset_mode = mode;
    }
    if args.no_gap_end_extend {
        config.repair.gap_end_extend_on_post_seam_fail = false;
    }
    if args.no_gap_start_extend {
        config.repair.gap_start_extend_on_pre_seam_fail = false;
    }
    if args.no_short_gap_one_strong_seam {
        config.repair.short_gap_one_strong_seam_fallback = false;
    }
    if let Some(ms) = args.gap_end_extend_max_ms {
        config.repair.gap_end_extend_max_ms = ms;
    }
    if let Some(ms) = args.gap_end_extend_step_ms {
        config.repair.gap_end_extend_step_ms = ms;
    }
    if let Some(ms) = args.crossfade_ms {
        config.repair.crossfade_ms = ms;
    }
    if let Some(num_clips) = args.num_clips {
        config.align.clip.num_clips = num_clips;
    }
    if args.verbose {
        config.logging.progress = ProgressMode::Verbose;
    }
    if args.quiet {
        config.logging.progress = ProgressMode::Quiet;
    }
    if let Some(level) = args.log_level {
        config.logging.level = level.into();
    }
    if let Some(path) = &args.log_file {
        config.logging.log_file = Some(path.clone());
    }
    if args.try_all_tracks {
        config.align.alignment.try_all_tracks = true;
    } else if args.no_try_all_tracks {
        config.align.alignment.try_all_tracks = false;
    }
    if args.refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = true;
    } else if args.no_refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = false;
    }
    if args.constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = true;
    } else if args.no_constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = false;
    }
    if args.high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = true;
    } else if args.no_high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = false;
    }
    if args.query_reference {
        config.align.alignment.mode = AlignmentMode::QueryReference;
    } else if args.symmetric_align {
        config.align.alignment.mode = AlignmentMode::Symmetric;
    }
    if let Some(stride) = args.query_stride {
        config.align.alignment.query_search_stride_secs = stride;
    }
    if args.no_limit_fill_region {
        config.repair.limit_fill_to_mapped_region = false;
    }
}

#[cfg(test)]
mod cli_override_tests {
    use super::*;
    use clip_sync::AlignmentMode;
    use crate::domain::FillOffsetMode;
    use crate::infrastructure::config::RepairAppConfig;

    #[test]
    fn query_reference_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--query-reference",
            "--query-stride",
            "45",
            "--no-limit-fill-region",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.align.alignment.mode, AlignmentMode::QueryReference);
        assert!((config.align.alignment.query_search_stride_secs - 45.0).abs() < f64::EPSILON);
        assert!(!config.repair.limit_fill_to_mapped_region);
    }

    #[test]
    fn no_structure_trust_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--no-structure-trust",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert!(config.repair.disable_structure_trust);
    }

    #[test]
    fn patch_and_scan_cli_overrides_config() {
        use clap::Parser;

        let args = Args::parse_from([
            "clip-sync-repair",
            "a.wav",
            "b.wav",
            "--silence-hold-ms",
            "400",
            "--absolute-silence-rms",
            "25",
            "--min-fill-correlation",
            "0.45",
            "--max-fill-align-adjust-secs",
            "0.25",
            "--border-standoff-secs",
            "0.5",
            "--fill-offset",
            "interpolated",
            "--no-gap-end-extend",
            "--no-gap-start-extend",
            "--no-short-gap-one-strong-seam",
            "--gap-end-extend-max-ms",
            "300",
            "--gap-end-extend-step-ms",
            "10",
        ]);
        let mut config = RepairAppConfig::default();
        apply_cli_overrides(&mut config, &args);
        assert_eq!(config.repair.silence_hold_ms, 400);
        assert!((config.repair.absolute_silence_rms - 25.0).abs() < f32::EPSILON);
        assert!((config.repair.min_fill_correlation - 0.45).abs() < f32::EPSILON);
        assert!((config.repair.max_fill_align_adjustment_secs - 0.25).abs() < f64::EPSILON);
        assert!((config.repair.border_standoff_secs - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.repair.fill_offset_mode, FillOffsetMode::Interpolated);
        assert!(!config.repair.gap_end_extend_on_post_seam_fail);
        assert!(!config.repair.gap_start_extend_on_pre_seam_fail);
        assert!(!config.repair.short_gap_one_strong_seam_fallback);
        assert_eq!(config.repair.gap_end_extend_max_ms, 300);
        assert_eq!(config.repair.gap_end_extend_step_ms, 10);
    }
}
