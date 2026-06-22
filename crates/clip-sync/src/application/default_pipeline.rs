use crate::application::align_videos::{AlignVideos, AlignVideosRequest};
use crate::application::error::AppError;
use crate::application::ports::ProgressReporter;
use crate::domain::AlignmentResult;
use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintClipRepetitionDetector, ChromaprintFingerprinter};
use crate::infrastructure::correlation::FftCorrelator;
use crate::infrastructure::resample::RubatoResampler;
use crate::infrastructure::symphonia::SymphoniaMediaReader;

pub fn align_with_defaults(
    request: AlignVideosRequest,
    progress: &dyn ProgressReporter,
) -> Result<AlignmentResult, AppError> {
    let preset = request.config.clip.chromaprint_preset;
    let media_reader = SymphoniaMediaReader;
    let fingerprinter = ChromaprintFingerprinter::new(preset);
    let aligner = ChromaprintAligner::new(preset);
    let resampler = RubatoResampler;
    let correlator = FftCorrelator;
    let repetition_detector = ChromaprintClipRepetitionDetector;
    let use_case = AlignVideos::new(
        &media_reader,
        &fingerprinter,
        &aligner,
        &resampler,
        &correlator,
        &repetition_detector,
        progress,
    );
    use_case.execute(request).map(|r| r.result)
}
