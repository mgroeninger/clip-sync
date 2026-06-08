use crate::application::align_videos::{AlignVideos, AlignVideosRequest, AlignVideosResponse};
use crate::application::error::AppError;
use crate::application::ports::ProgressReporter;
use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
use crate::infrastructure::symphonia::SymphoniaMediaReader;

pub fn align_with_defaults(
    request: AlignVideosRequest,
    progress: &dyn ProgressReporter,
) -> Result<AlignVideosResponse, AppError> {
    let preset = request.config.clip.chromaprint_preset;
    let media_reader = SymphoniaMediaReader;
    let fingerprinter = ChromaprintFingerprinter::new(preset);
    let aligner = ChromaprintAligner::new(preset);
    let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, progress);
    use_case.execute(request)
}
