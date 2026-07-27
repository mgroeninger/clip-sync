use std::time::Duration;

/// Declared container duration vs tail-scanned decodable extent for one audio track.
///
/// `effective()` prefers `decodable` when known, always clamped to `declared`. Declared
/// duration remains the ceiling — planning windows beyond it is high risk (seeks go
/// `OutOfRange`) and is not supported today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaExtent {
    pub declared: Duration,
    pub decodable: Option<Duration>,
}

impl MediaExtent {
    pub fn new(declared: Duration, decodable: Option<Duration>) -> Self {
        Self {
            declared,
            decodable: decodable.map(|value| value.min(declared)),
        }
    }

    pub fn from_declared(declared: Duration) -> Self {
        Self {
            declared,
            decodable: None,
        }
    }

    /// Duration for clip planning, hold-out placement, and feasibility checks.
    pub fn effective(&self) -> Duration {
        self.decodable.unwrap_or(self.declared).min(self.declared)
    }

    pub fn with_decodable(self, decodable: Option<Duration>) -> Self {
        Self::new(self.declared, decodable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_uses_declared_when_decodable_unknown() {
        let extent = MediaExtent::from_declared(Duration::from_secs(100));
        assert_eq!(extent.effective(), Duration::from_secs(100));
    }

    #[test]
    fn effective_uses_decodable_when_shorter() {
        let extent = MediaExtent::new(Duration::from_secs(100), Some(Duration::from_secs(60)));
        assert_eq!(extent.effective(), Duration::from_secs(60));
    }

    #[test]
    fn effective_clamps_decodable_to_declared() {
        let extent = MediaExtent::new(Duration::from_secs(30), Some(Duration::from_secs(60)));
        assert_eq!(extent.effective(), Duration::from_secs(30));
    }

    #[test]
    fn with_decodable_clamps_to_declared() {
        let extent = MediaExtent::from_declared(Duration::from_secs(10))
            .with_decodable(Some(Duration::from_secs(20)));
        assert_eq!(extent.decodable, Some(Duration::from_secs(10)));
    }
}
