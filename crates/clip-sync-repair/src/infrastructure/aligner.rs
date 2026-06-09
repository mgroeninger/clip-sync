use clip_sync::{
    align_with_defaults, AlignVideosRequest, AlignmentResult, AppError, ProgressReporter,
};

use crate::application::ports::Aligner;

pub struct SymphoniaAligner;

impl Aligner for SymphoniaAligner {
    fn align(
        &self,
        request: AlignVideosRequest,
        progress: &dyn ProgressReporter,
    ) -> Result<AlignmentResult, AppError> {
        align_with_defaults(request, progress)
    }
}
