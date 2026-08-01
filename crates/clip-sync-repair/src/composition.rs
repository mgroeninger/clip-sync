//! Binary composition root: default adapter wiring and config → use-case mapping.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clip_sync::{ProgressReporter, SymphoniaMediaReader};

use crate::application::error::RepairError;
#[cfg(feature = "calibration")]
use crate::application::patch_audio::decode_ab;
use crate::application::run_repair::{
    run_repair, PendingAfterScan, PendingRepairWrite, RepairRunInput, RepairRunOutcome,
};
use crate::application::scan_gaps::ScanGapsRequest;
#[cfg(feature = "calibration")]
use crate::domain::GapReport;
use crate::infrastructure::aligner::SymphoniaAligner;
use crate::infrastructure::cli::{
    self, args::Args, exit_code::exit_code_for, output::print_repair_output,
    validate_fingerprint_flags, validate_repair_profile_flags,
};
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
    validate_repair_profile_flags(&args).map_err(RepairError::Config)?;
    validate_fingerprint_flags(&args).map_err(RepairError::Config)?;
    cli::apply_cli_overrides(&mut config, &args);
    validate_config(&config)?;

    clip_sync::init_tracing(&config.logging).map_err(RepairError::Align)?;

    let progress = clip_sync::StderrProgressReporter::new(config.logging.progress);
    progress.phase(&format!(
        "clip-sync-repair: aligning {} with {}",
        args.video_a.display(),
        args.video_b.display()
    ));

    let input = repair_run_input(&config, args.video_a.clone(), args.video_b.clone())?;
    let outcome = run_repair_with_defaults(input, &progress)?;

    #[cfg(feature = "calibration")]
    if let Some(dir) = args.gap_fingerprints.clone() {
        // Repair takes priority: a real repair (--mux) wins over the fingerprint diagnostic.
        if args.mux.is_some() {
            eprintln!(
                "warning: --mux takes priority; ignoring --gap-fingerprints / --fingerprint-gap"
            );
        } else {
            dump_gap_fingerprints(&args, &config, &outcome.report, &progress, &dir)?;
        }
    }

    print_repair_outcome(&args, &config, outcome)
}

/// Convert 1-based `--fingerprint-gap` numbers (the repair gap table's `#`) into the 0-based
/// positions in `GapReport.gaps` that `characterize_gaps_from_decode` expects.
///
/// This is the only base conversion for the flag: everything downstream — `select`,
/// `GapFingerprint::index`, the `g{:03}` filename segment — stays 0-based. `0` is rejected explicitly
/// because `n - 1` would underflow a `usize` rather than fail.
///
/// Errors say *gap number*, never "gap index": "index" is reserved for the 0-based internal axis
/// (`FillRegion::gap_index`), so an error reading "gap indices are 1-based" would contradict it.
#[cfg(feature = "calibration")]
fn resolve_fingerprint_gap_select(
    gap_numbers: &[usize],
    gap_count: usize,
) -> Result<Vec<usize>, RepairError> {
    gap_numbers
        .iter()
        .map(|&n| match n {
            0 => Err(RepairError::Config(
                "gap number 0 is invalid (gap numbers are 1-based)".to_string(),
            )),
            n if n > gap_count => Err(RepairError::Config(format!(
                "gap number {n} out of range ({gap_count} gaps detected)"
            ))),
            n => Ok(n - 1),
        })
        .collect()
}

/// Diagnostic: decode A/B (shared `decode_ab`), characterize each gap, and write a licensing-safe
/// corpus directory (`corpus.json` + per-gap files + `manifest.json`). Gaps named via
/// `--fingerprint-gap` get full detail (per-bracket gate `failure_stage` + lag); the rest summary.
#[cfg(feature = "calibration")]
fn dump_gap_fingerprints(
    args: &Args,
    config: &RepairAppConfig,
    report: &GapReport,
    progress: &dyn ProgressReporter,
    dir: &std::path::Path,
) -> Result<(), RepairError> {
    progress.phase("characterizing gaps for fingerprinting");
    let select = resolve_fingerprint_gap_select(&args.fingerprint_gap, report.gaps.len())?;
    let media_reader = SymphoniaMediaReader;
    let decoded = decode_ab(&media_reader, report, progress)?;
    let request = config
        .repair
        .patch_settings()
        .into_request(report.clone())
        .map_err(RepairError::GapSelection)?;

    let dump = |sub: &str,
                corpus: crate::application::gap_fingerprint::GapCorpus|
     -> Result<(), RepairError> {
        let out = if sub.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(sub)
        };
        let n =
            crate::application::gap_fingerprint::write_corpus_dir(&corpus, &out).map_err(|e| {
                RepairError::Config(format!("write gap corpus to {}: {e}", out.display()))
            })?;
        progress.phase(&format!(
            "wrote corpus.json + {n} per-gap fingerprints + manifest.json to {}",
            out.display()
        ));
        Ok(())
    };
    // The dump is characterized from decode via the shared projection (8g.4a/8g.4b); the old per-bracket
    // oracle path was removed at 8g.6.
    let corpus = crate::application::gap_fingerprint::characterize_gaps_from_decode(
        report,
        &crate::application::gap_fingerprint::CharacterizeAbPcm {
            a_pcm: &decoded.a_pcm,
            b_samples: &decoded.b_samples_full,
            sources: Some(&decoded.sources),
        },
        &request,
        &select,
        config.repair.fingerprint_diagnostics,
        progress,
    );
    if let Some(reason) = corpus.source.incomparable {
        use crate::application::gap_fingerprint::IncomparableReason;
        match reason {
            IncomparableReason::ChannelLayoutMismatch => {
                progress.phase(&format!(
                    "channel layout mismatch (A {}ch / B {}ch): refused pairwise fingerprint \
                     characterize; writing provenance-only corpus (gaps empty)",
                    decoded.sources.a.native_channels, decoded.sources.b.native_channels,
                ));
            }
        }
    }
    dump("", corpus)?;
    Ok(())
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
    let scan = ScanGapsRequest {
        video_a: video_a.clone(),
        video_b,
        align: config.align.clone(),
        decode_chunk_secs: config.repair.decode_chunk_secs,
        recipe: crate::domain::ScanRecipe::with_hold_blocks(
            config.repair.min_gap_ms,
            config.repair.silence_hold_blocks(),
            config.repair.scan_block_ms,
            config.repair.silence_peak_fraction,
            config.repair.absolute_silence_rms,
        ),
        scan_both: config.repair.scan_both,
        gap_offset_tolerance_secs: config.repair.gap_offset_tolerance_secs,
        limit_fill_to_mapped_region: config.repair.limit_fill_to_mapped_region,
    };

    let after_scan = pending_after_scan(config, video_a)?;
    Ok(RepairRunInput { scan, after_scan })
}

/// Map `RepairConfig` run-mode fields (`dry_run`, `repair_preview`, output paths) onto the
/// post-scan orchestration arm.
fn pending_after_scan(
    config: &RepairAppConfig,
    source_video: PathBuf,
) -> Result<PendingAfterScan, RepairError> {
    if config.repair.repair_preview {
        return Ok(PendingAfterScan::Preview {
            patch_settings: config.repair.patch_settings(),
            crossfade_ms: config.repair.crossfade_ms,
        });
    }

    match pending_repair_write(config, source_video)? {
        Some(write) => Ok(PendingAfterScan::Write(write)),
        None => Ok(PendingAfterScan::None {
            gap_selection: config.repair.gap_selection_mode(),
        }),
    }
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
        .filter(|result| !result.preview && result.summary.has_patches())
        .and({
            #[cfg(feature = "ffmpeg-mux")]
            {
                config.repair.output.video_path.as_deref().or(config
                    .repair
                    .output
                    .wav_path
                    .as_deref())
            }
            #[cfg(not(feature = "ffmpeg-mux"))]
            {
                config.repair.output.wav_path.as_deref()
            }
        });

    let skip_json = suppress_json_on_gap_selection_error(args.format, &outcome.patch_result);
    if !skip_json {
        print_repair_output(
            &outcome.report,
            &outcome.alignment_detail,
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
    }

    outcome.patch_result.map(|_| ())
}

/// Under `--format json`, a post-scan selection failure must not emit a success-shaped document.
fn suppress_json_on_gap_selection_error<T>(
    format: crate::infrastructure::config::OutputFormat,
    patch_result: &Result<T, RepairError>,
) -> bool {
    matches!(
        (format, patch_result),
        (
            crate::infrastructure::config::OutputFormat::Json,
            Err(RepairError::GapSelection(_))
        )
    )
}

#[cfg(test)]
mod gap_selection_output_tests {
    use super::{suppress_json_on_gap_selection_error, RepairError};
    use crate::infrastructure::config::OutputFormat;

    #[test]
    fn json_format_suppresses_stdout_on_gap_selection_error() {
        let err: Result<(), RepairError> = Err(RepairError::GapSelection(
            "gap number 9 out of range".into(),
        ));
        assert!(suppress_json_on_gap_selection_error(
            OutputFormat::Json,
            &err
        ));
        assert!(!suppress_json_on_gap_selection_error(
            OutputFormat::Human,
            &err
        ));
        let ok: Result<(), RepairError> = Ok(());
        assert!(!suppress_json_on_gap_selection_error(
            OutputFormat::Json,
            &ok
        ));
        let config_err: Result<(), RepairError> = Err(RepairError::Config("other".into()));
        assert!(!suppress_json_on_gap_selection_error(
            OutputFormat::Json,
            &config_err
        ));
    }
}

#[cfg(all(test, feature = "calibration"))]
mod tests {
    use super::{resolve_fingerprint_gap_select, RepairError};

    fn message(err: RepairError) -> String {
        match err {
            RepairError::Config(msg) => msg,
            other => panic!("expected a config error, got {other:?}"),
        }
    }

    #[test]
    fn fingerprint_gap_numbers_convert_to_zero_based_positions() {
        // `--fingerprint-gap 3` is the table's `#3`, i.e. `report.gaps[2]` — and the corpus file it
        // writes is `..._g002_....json`. Order is preserved; nothing is deduplicated here.
        assert_eq!(
            resolve_fingerprint_gap_select(&[3, 1, 6], 6).expect("in range"),
            vec![2, 0, 5]
        );
        assert_eq!(
            resolve_fingerprint_gap_select(&[], 6).expect("empty selects all downstream"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn fingerprint_gap_zero_is_rejected_rather_than_underflowing() {
        let err = message(resolve_fingerprint_gap_select(&[1, 0], 6).expect_err("0 is invalid"));
        assert_eq!(err, "gap number 0 is invalid (gap numbers are 1-based)");
    }

    #[test]
    fn fingerprint_gap_out_of_range_names_the_detected_count() {
        let err = message(resolve_fingerprint_gap_select(&[7], 6).expect_err("out of range"));
        assert_eq!(err, "gap number 7 out of range (6 gaps detected)");
        // The last valid number is the gap count itself, not `count - 1`.
        assert_eq!(
            resolve_fingerprint_gap_select(&[6], 6).expect("in range"),
            vec![5]
        );
    }
}
