//! Binary composition root: wires the analyzer use case after CLI config is resolved.

use std::process::ExitCode;

use clap::Parser;
use clip_sync::{init_tracing, AppError, ProgressReporter, StderrProgressReporter};

use crate::application::run_align::run_align;
use crate::infrastructure::cli::{self, args::Cli, exit_code, output};
use crate::infrastructure::config::load_app_config;

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
    cli::apply_cli_overrides(&mut config, &cli);
    config.validate()?;

    init_tracing(&config.logging)?;

    let progress = StderrProgressReporter::new(config.logging.progress);
    progress.phase(&format!(
        "clip-sync: aligning {} with {}",
        cli.video_a.display(),
        cli.video_b.display()
    ));
    let result = run_align(&config.align, cli.video_a, cli.video_b, &progress)?;

    output::print_success(&config.output, &result)
}
