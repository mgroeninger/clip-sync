//! Shared test stubs used by repair unit tests and `clip-sync-repair-fixtures`.

use crate::application::ports::Aligner;

/// Aligner stub for tests that call `scan_after_alignment` (or inject alignment) directly.
///
/// Lives in the repair crate (not fixtures) so unit tests and fixtures share one `Aligner`
/// impl without hitting the duplicate-crate trait bound when fixtures is a repair `[dev-dependency]`.
#[doc(hidden)]
pub struct NeverCalledAligner;

impl Aligner for NeverCalledAligner {
    fn align(
        &self,
        _: clip_sync::AlignVideosRequest,
        _: &dyn clip_sync::ProgressReporter,
    ) -> Result<clip_sync::AlignmentResult, clip_sync::AppError> {
        unreachable!("tests use scan_after_alignment (or inject alignment) directly")
    }
}
