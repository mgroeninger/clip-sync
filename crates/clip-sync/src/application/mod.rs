pub mod align_videos;
pub mod config;
pub mod default_pipeline;
pub mod error;
pub mod extraction_progress;
pub mod high_rate_refinement;
pub mod locate_query;
#[cfg(test)]
mod locate_query_spike;
pub mod media_scan;
pub mod offset_refinement;
pub mod offset_verification;
pub mod ports;
pub mod report;

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

pub use align_videos::{AlignVideos, AlignVideosRequest, AlignVideosResponse};
pub use error::{AppError, ConfigError};
