use std::process::ExitCode;

use clap::Parser;

use crate::application::config::{AppConfig, ProgressMode};
use crate::application::error::AppError;
use crate::application::{AlignVideos, AlignVideosRequest};
use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
use crate::infrastructure::config::file::load_optional_config_file;
use crate::infrastructure::logging::{init_tracing, StderrProgressReporter};
use crate::infrastructure::symphonia::SymphoniaMediaReader;

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
    let mut config = load_optional_config_file(cli.config.as_deref())?;
    apply_cli_overrides(&mut config, &cli);
    config.validate()?;

    init_tracing(&config.logging)?;

    let progress = StderrProgressReporter::new(config.logging.progress);
    let media_reader = SymphoniaMediaReader;
    let preset = config.clip.chromaprint_preset;
    let fingerprinter = ChromaprintFingerprinter::new(preset);
    let aligner = ChromaprintAligner::new(preset);

    let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);
    let response = use_case.execute(AlignVideosRequest {
        video_a: cli.video_a,
        video_b: cli.video_b,
        config: config.clone(),
    })?;

    output::print_success(&config.output, &response.result)
}

fn apply_cli_overrides(config: &mut AppConfig, cli: &Cli) {
    if let Some(duration) = cli.clip_length {
        config.clip.clip_length = duration;
    }
    if let Some(num_clips) = cli.num_clips {
        config.clip.num_clips = num_clips;
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
        config.alignment.try_all_tracks = true;
    }
}
