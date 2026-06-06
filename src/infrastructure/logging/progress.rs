use std::io::{IsTerminal, Write};

use crate::application::config::ProgressMode;
use crate::application::ports::ProgressReporter;

pub struct StderrProgressReporter {
    mode: ProgressMode,
    is_tty: bool,
}

impl StderrProgressReporter {
    pub fn new(mode: ProgressMode) -> Self {
        Self {
            mode,
            is_tty: std::io::stderr().is_terminal(),
        }
    }

    fn enabled(&self) -> bool {
        !matches!(self.mode, ProgressMode::Quiet)
    }

    fn show_progress_bar(&self) -> bool {
        matches!(self.mode, ProgressMode::Verbose) || self.is_tty
    }
}

impl ProgressReporter for StderrProgressReporter {
    fn phase(&self, message: &str) {
        if self.enabled() {
            let _ = writeln!(std::io::stderr(), "{message}");
        }
    }

    fn progress(&self, label: &str, current: u64, total: u64) {
        if !self.enabled() || !self.show_progress_bar() || total == 0 {
            return;
        }

        let percent = (current * 100) / total;
        let _ = writeln!(std::io::stderr(), "{label}: {percent}%");
    }
}
