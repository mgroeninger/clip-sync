use std::path::PathBuf;

use clip_sync::{align_with_defaults, AlignVideosRequest, AlignmentResult, AppError, ProgressReporter};

use crate::infrastructure::config::AppConfig;

pub fn run_align(
    config: &AppConfig,
    video_a: PathBuf,
    video_b: PathBuf,
    progress: &dyn ProgressReporter,
) -> Result<AlignmentResult, AppError> {
    align_with_defaults(
        AlignVideosRequest {
            video_a,
            video_b,
            config: config.align.clone(),
        },
        progress,
    )
}
