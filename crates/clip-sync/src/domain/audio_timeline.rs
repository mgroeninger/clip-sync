use serde::Serialize;

/// Observed divergence between packet PTS and the sequential decoded-sample clock.
///
/// Gap scan uses the sample clock; patch extract maps samples by PTS. A large delta usually
/// indicates container timestamp issues (e.g. sloppy remux or AAC priming).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AudioTimelineSkew {
    pub pts_secs: f64,
    pub sample_clock_secs: f64,
    pub delta_secs: f64,
}
