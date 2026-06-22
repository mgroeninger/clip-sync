use std::path::PathBuf;

use clip_sync::{
    align_with_defaults, AlignConfig, AlignVideosRequest, AlignmentResult, AppError,
    ProgressReporter,
};

pub fn run_align(
    align: &AlignConfig,
    video_a: PathBuf,
    video_b: PathBuf,
    progress: &dyn ProgressReporter,
) -> Result<AlignmentResult, AppError> {
    align_with_defaults(
        AlignVideosRequest {
            video_a,
            video_b,
            config: align.clone(),
        },
        progress,
    )
}
