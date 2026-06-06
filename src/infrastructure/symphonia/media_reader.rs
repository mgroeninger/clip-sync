use std::time::Duration;

use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{AudioTrack, ClipWindow, MediaSource, MonoPcmClip};

pub struct SymphoniaMediaReader;

impl MediaReader for SymphoniaMediaReader {
    type Session = SymphoniaMediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError> {
        if !source.path().exists() {
            return Err(MediaError::FileNotFound(source.path().to_path_buf()));
        }

        Err(MediaError::OpenFailed(format!(
            "Symphonia media reader not yet implemented for {}",
            source.path().display()
        )))
    }
}

pub struct SymphoniaMediaSession;

impl MediaSession for SymphoniaMediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
        Err(MediaError::OpenFailed("not implemented".into()))
    }

    fn duration(&self) -> Result<Duration, MediaError> {
        Err(MediaError::OpenFailed("not implemented".into()))
    }

    fn extract_mono(
        &self,
        _track: &AudioTrack,
        _window: &ClipWindow,
        _progress: &dyn ProgressReporter,
        _label: &str,
    ) -> Result<MonoPcmClip, MediaError> {
        Err(MediaError::DecodeFailed {
            track: 0,
            detail: "not implemented".into(),
        })
    }
}
