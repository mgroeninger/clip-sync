use std::cell::Cell;
use std::io::{IsTerminal, Write};

use crate::infrastructure::logging::ProgressMode;
use crate::application::ports::ProgressReporter;

pub struct StderrProgressReporter {
    mode: ProgressMode,
    is_tty: bool,
    progress_active: Cell<bool>,
    last_percent: Cell<Option<u64>>,
}

impl StderrProgressReporter {
    pub fn new(mode: ProgressMode) -> Self {
        Self {
            mode,
            is_tty: std::io::stderr().is_terminal(),
            progress_active: Cell::new(false),
            last_percent: Cell::new(None),
        }
    }

    fn enabled(&self) -> bool {
        !matches!(self.mode, ProgressMode::Quiet)
    }

    fn show_progress_bar(&self) -> bool {
        matches!(self.mode, ProgressMode::Verbose) || self.is_tty
    }

    fn finish_progress_line(&self, stderr: &mut impl Write) {
        if self.is_tty && self.progress_active.get() {
            let _ = writeln!(stderr);
            self.progress_active.set(false);
            self.last_percent.set(None);
        }
    }
}

impl ProgressReporter for StderrProgressReporter {
    fn phase(&self, message: &str) {
        if !self.enabled() {
            return;
        }

        let mut stderr = std::io::stderr().lock();
        self.finish_progress_line(&mut stderr);
        let _ = writeln!(stderr, "{message}");
    }

    fn progress(&self, label: &str, current: u64, total: u64) {
        if !self.enabled() || !self.show_progress_bar() || total == 0 {
            return;
        }

        let percent = (current * 100) / total;
        let mut stderr = std::io::stderr().lock();

        if self.is_tty {
            let _ = write!(stderr, "\r{label}: {percent}%");
            let _ = stderr.flush();
            self.progress_active.set(true);
            self.last_percent.set(Some(percent));

            if current >= total {
                let _ = writeln!(stderr);
                self.progress_active.set(false);
                self.last_percent.set(None);
            }
            return;
        }

        if self.last_percent.get() == Some(percent) {
            return;
        }
        self.last_percent.set(Some(percent));
        let _ = writeln!(stderr, "{label}: {percent}%");

        if current >= total {
            self.last_percent.set(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_mode_suppresses_output() {
        let reporter = StderrProgressReporter::new(ProgressMode::Quiet);
        assert!(!reporter.enabled());
        assert!(!reporter.show_progress_bar());
    }
}
