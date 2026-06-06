use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("no audio tracks found")]
    NoAudioTracks,
    #[error("invalid media duration")]
    InvalidDuration,
    #[error("empty clip")]
    EmptyClip,
}
