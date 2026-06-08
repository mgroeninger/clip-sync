mod application;
mod infrastructure;

use std::process::ExitCode;

fn main() -> ExitCode {
    infrastructure::cli::run()
}
