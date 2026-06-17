#[cfg(any(test, feature = "test-utils"))]
use std::time::Duration;

#[cfg(any(test, feature = "test-utils"))]
use crate::domain::{
    AlignmentResult, ClipLabel, ClipMatch, HighRateRefinement, OffsetVerification,
};

/// Builder for [`AlignmentResult`] in unit / CLI tests.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Clone)]
pub struct AlignmentResultBuilder {
    result: AlignmentResult,
}

#[cfg(any(test, feature = "test-utils"))]
impl AlignmentResultBuilder {
    pub fn new(recommended_offset_secs: Option<f64>) -> Self {
        let aligned = recommended_offset_secs.is_some();
        Self {
            result: AlignmentResult {
                clips: vec![ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 60.0,
                    aligned,
                    offset_secs: recommended_offset_secs,
                    confidence: if aligned { 0.9 } else { 0.0 },
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                }],
                start_aligned: aligned,
                end_aligned: None,
                recommended_offset_secs,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                high_rate_refinement: None,
                offset_verification: None,
                offset_ambiguous_mod_secs: None,
                alignment_mode_used: None,
                query_localization: None,
                end_clip_anchor: None,
            },
        }
    }

    pub fn with_offset(mut self, recommended_offset_secs: Option<f64>) -> Self {
        let aligned = recommended_offset_secs.is_some();
        self.result.recommended_offset_secs = recommended_offset_secs;
        self.result.start_aligned = aligned;
        if let Some(start) = self
            .result
            .clips
            .iter_mut()
            .find(|clip| clip.label == ClipLabel::Start)
        {
            start.aligned = aligned;
            start.offset_secs = recommended_offset_secs;
        }
        self
    }

    pub fn with_clips(mut self, clips: Vec<ClipMatch>) -> Self {
        self.result.clips = clips;
        self
    }

    pub fn with_verification(mut self, offset_verification: Option<OffsetVerification>) -> Self {
        self.result.offset_verification = offset_verification;
        self
    }

    pub fn with_high_rate_refinement(
        mut self,
        high_rate_refinement: Option<HighRateRefinement>,
    ) -> Self {
        self.result.high_rate_refinement = high_rate_refinement;
        self
    }

    pub fn build(self) -> AlignmentResult {
        self.result
    }
}

/// Minimal [`AlignmentResult`] for unit tests.
#[cfg(any(test, feature = "test-utils"))]
pub fn minimal_alignment_result(recommended_offset_secs: Option<f64>) -> AlignmentResultBuilder {
    AlignmentResultBuilder::new(recommended_offset_secs)
}

/// Start-window clip with the fields most CLI / report tests customize.
#[cfg(any(test, feature = "test-utils"))]
pub fn start_clip_match(
    offset_secs: Option<f64>,
    window_end_secs: f64,
    confidence: f32,
) -> ClipMatch {
    let aligned = offset_secs.is_some();
    ClipMatch {
        label: ClipLabel::Start,
        window_start_secs: 0.0,
        window_end_secs,
        aligned,
        offset_secs,
        confidence,
        video_a_decode_skips: 0,
        video_b_decode_skips: 0,
        repetition: None,
        video_b_window_start_secs: None,
        video_b_window_end_secs: None,
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub fn mins(m: u64) -> Duration {
    Duration::from_secs(m * 60)
}
