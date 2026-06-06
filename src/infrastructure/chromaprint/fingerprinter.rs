use crate::application::error::FingerprintError;
use crate::application::ports::Fingerprinter;
use crate::domain::{Fingerprint, MonoPcmClip};

pub struct ChromaprintFingerprinter;

impl Fingerprinter for ChromaprintFingerprinter {
    fn fingerprint(&self, _clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError> {
        Err(FingerprintError::EngineFailed(
            "Chromaprint fingerprinter not yet implemented".into(),
        ))
    }
}
