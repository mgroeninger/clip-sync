use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipPlan {
    pub clip_length: Duration,
    pub num_clips: u32,
}

impl ClipPlan {
    pub fn new(clip_length: Duration, num_clips: u32) -> Self {
        Self {
            clip_length,
            num_clips,
        }
    }
}
