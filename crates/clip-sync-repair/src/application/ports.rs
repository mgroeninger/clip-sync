use crate::application::error::RepairError;
use crate::domain::GapReport;

/// Output port: formats and emits a gap report (human or JSON).
pub trait GapReporter {
    fn report(&self, report: &GapReport) -> Result<(), RepairError>;
}

/// Output port stub for Phase 5: muxes replacement audio into video.
/// Not implemented until Phase 5; defined here to keep the port layer complete.
pub trait MediaMuxer {
    // Phase 5: fn mux_video_with_replaced_audio(...) -> Result<(), MuxError>;
}
