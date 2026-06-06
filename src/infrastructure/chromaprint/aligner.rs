pub use crate::domain::AlignmentResult;

use crate::application::error::AlignmentError;
use crate::application::ports::Aligner;
use crate::domain::Fingerprint;

pub struct ChromaprintAligner;

impl Aligner for ChromaprintAligner {
    fn find_offset(
        &self,
        _left: &Fingerprint,
        _right: &Fingerprint,
    ) -> Result<AlignmentResult, AlignmentError> {
        Err(AlignmentError::EngineFailed(
            "Chromaprint aligner not yet implemented".into(),
        ))
    }
}
