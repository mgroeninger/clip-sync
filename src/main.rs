mod application;
mod domain;
mod infrastructure;

use std::process::ExitCode;

fn main() -> ExitCode {
    infrastructure::cli::run()
}
