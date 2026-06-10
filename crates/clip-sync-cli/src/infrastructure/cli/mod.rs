use std::process::ExitCode;

use clap::Parser;

use clip_sync::{AppError, ProgressMode, ProgressReporter, init_tracing, StderrProgressReporter};

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
    if cli.check_clip_repetition {
        config.align.validation.check_clip_repetition = true;
    }
}
