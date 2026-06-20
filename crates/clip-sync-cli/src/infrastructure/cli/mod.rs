use std::process::ExitCode;

use clap::Parser;

use clip_sync::{AppError, AlignmentMode, ProgressMode, ProgressReporter, init_tracing, StderrProgressReporter};

use crate::application::run_align::run_align;
use crate::infrastructure::config::{AppConfig, load_app_config};

pub mod args;
pub mod exit_code;
pub mod output;

use args::Cli;

pub fn run() -> ExitCode {
    match run_inner() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::debug!(error = %error, "clip-sync failed");
            eprintln!("{error}");
            exit_code::exit_code_for(&error)
        }
    }
}

fn run_inner() -> Result<(), AppError> {
    let cli = Cli::parse();
    let mut config = load_app_config(cli.config.as_deref())?;
    apply_cli_overrides(&mut config, &cli);
    config.validate()?;

    init_tracing(&config.logging)?;

    let progress = StderrProgressReporter::new(config.logging.progress);
    progress.phase(&format!(
        "clip-sync: aligning {} with {}",
        cli.video_a.display(),
        cli.video_b.display()
    ));
    let result = run_align(&config, cli.video_a, cli.video_b, &progress)?;

    output::print_success(&config.output, &result)
}

fn apply_cli_overrides(config: &mut AppConfig, cli: &Cli) {
    if let Some(duration) = cli.clip_length {
        config.align.clip.clip_length = duration;
    }
    if let Some(num_clips) = cli.num_clips {
        config.align.clip.num_clips = num_clips;
    }
    if let Some(format) = cli.format {
        config.output.format = format.into();
    }
    if cli.verbose {
        config.output.show_diagnostics = true;
        config.logging.progress = ProgressMode::Verbose;
    }
    if cli.quiet {
        config.logging.progress = ProgressMode::Quiet;
    }
    if let Some(level) = cli.log_level {
        config.logging.level = level.into();
    }
    if let Some(path) = &cli.log_file {
        config.logging.log_file = Some(path.clone());
    }
    if cli.try_all_tracks {
        config.align.alignment.try_all_tracks = true;
    } else if cli.no_try_all_tracks {
        config.align.alignment.try_all_tracks = false;
    }
    if cli.refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = true;
    } else if cli.no_refine_offset_high_rate {
        config.align.alignment.refine_offset_high_rate = false;
    }
    if cli.constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = true;
    } else if cli.no_constrain_end_clip_to_start_offset {
        config.align.alignment.constrain_end_clip_to_start_offset = false;
    }
    if cli.high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = true;
    } else if cli.no_high_rate_recommended_refusion {
        config.align.alignment.high_rate_recommended_refusion = false;
    }
    if cli.check_clip_repetition {
        config.align.validation.check_clip_repetition = true;
    }
    if cli.verify_offset {
        config.align.validation.verify_offset = true;
    }
    if cli.query_reference {
        config.align.alignment.mode = AlignmentMode::QueryReference;
    } else if cli.symmetric_align {
        config.align.alignment.mode = AlignmentMode::Symmetric;
    }
    if let Some(stride) = cli.query_stride {
        config.align.alignment.query_search_stride_secs = stride;
    }
}

#[cfg(test)]
mod cli_override_tests {
    use super::*;
    use clap::Parser;
    use clip_sync::AlignmentMode;

    #[test]
    fn query_reference_cli_overrides_config() {
        let cli = Cli::parse_from([
            "clip-sync",
            "a.wav",
            "b.wav",
            "--query-reference",
            "--query-stride",
            "45",
        ]);
        let mut config = AppConfig::default();
        apply_cli_overrides(&mut config, &cli);
        assert_eq!(config.align.alignment.mode, AlignmentMode::QueryReference);
        assert!((config.align.alignment.query_search_stride_secs - 45.0).abs() < f64::EPSILON);
    }
}
