use crate::application::error::{AlignmentError, FingerprintError, MediaError};
use crate::domain::{
    AudioTrack, ClipMatchEstimate, ClipWindow, Fingerprint, MediaSource, MonoPcmClip,
};

pub trait ProgressReporter {
    fn phase(&self, message: &str);
    fn progress(&self, label: &str, current: u64, total: u64);
}

pub trait MediaReader {
    type Session: MediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError>;
}

pub trait MediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError>;
    fn extract_mono(
        &self,
        track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MonoPcmClip, MediaError>;

    /// Rewind the underlying format reader and drop cached decoders before a distant seek.
    fn reset_io(&self) -> Result<(), MediaError> {
        Ok(())
    }
}

pub trait Fingerprinter {
    fn fingerprint(&self, clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError>;
}

pub trait Aligner {
    /// Compare clip fingerprints from video A (`left`) and video B (`right`).
    /// Returns seconds to add to A's timeline to align with B (see PLAN.md).
    fn find_offset(
        &self,
        left: &Fingerprint,
        right: &Fingerprint,
    ) -> Result<ClipMatchEstimate, AlignmentError>;
}
