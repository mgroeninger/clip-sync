use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipLabel {
    Start,
    Interior,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipWindow {
    pub start: Duration,
    pub end: Duration,
    pub label: ClipLabel,
}

impl ClipWindow {
    pub fn new(start: Duration, end: Duration, label: ClipLabel) -> Self {
        Self { start, end, label }
    }

    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }
}
