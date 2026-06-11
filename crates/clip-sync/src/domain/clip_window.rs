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

    pub fn sample_count_at(&self, sample_rate: u32) -> usize {
        ((self.duration().as_secs_f64() * f64::from(sample_rate))
            .floor()
            .max(1.0)) as usize
    }
}
