use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("no audio tracks found")]
    NoAudioTracks,
    #[error("no decodable audio tracks")]
    NoDecodableAudioTracks,
    #[error("invalid media duration")]
    InvalidDuration,
    #[error("empty clip")]
    EmptyClip,
}
