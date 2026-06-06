use std::path::PathBuf;

use thiserror::Error;

use crate::domain::DomainError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Fingerprint(#[from] FingerprintError),
    #[error(transparent)]
    Alignment(#[from] AlignmentError),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MediaError {
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("failed to open media: {0}")]
    OpenFailed(String),
    #[error("decode failed on track {track}: {detail}")]
    DecodeFailed { track: u32, detail: String },
    #[error("seek failed: {0}")]
    SeekFailed(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    #[error("invalid PCM: {0}")]
    InvalidPcm(String),
    #[error("fingerprint engine failed: {0}")]
    EngineFailed(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AlignmentError {
    #[error("could not find a matching segment")]
    NoMatch,
    #[error("multiple equally likely matches ({candidates} candidates)")]
    AmbiguousMatch { candidates: usize },
    #[error("alignment engine failed: {0}")]
    EngineFailed(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    FileRead(PathBuf),
    #[error("failed to parse config: {0}")]
    Parse(String),
    #[error("invalid config value for `{field}`: {reason}")]
    InvalidValue { field: String, reason: String },
}
