use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use crate::domain::DomainError;

/// Type-erased underlying cause attached by adapter error mapping (reachable via `source()`).
///
/// `Arc` rather than `Box` so error values stay `Clone` — test fakes store an error and return
/// a clone per call. Adapters attach the original error (Symphonia, io, toml, chromaprint)
/// without leaking its type into the port signature.
///
/// The enums below implement `Display`/`Error` by hand instead of deriving `#[source]`:
/// `Arc<dyn Error>` itself implements `Error`, so thiserror would surface the `Arc` wrapper as
/// the chain node and the wrapped error's concrete type would not be downcastable. The manual
/// `source()` returns the inner error directly.
pub type ErrorSource = Arc<dyn std::error::Error + Send + Sync + 'static>;

fn as_dyn_source(source: &Option<ErrorSource>) -> Option<&(dyn std::error::Error + 'static)> {
    source.as_deref().map(|error| error as _)
}

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

#[derive(Debug, Clone)]
pub enum MediaError {
    FileNotFound(PathBuf),
    UnsupportedFormat {
        detail: String,
        source: Option<ErrorSource>,
    },
    OpenFailed {
        detail: String,
        source: Option<ErrorSource>,
    },
    DecodeFailed {
        track: u32,
        detail: String,
        source: Option<ErrorSource>,
    },
    SeekFailed {
        detail: String,
        source: Option<ErrorSource>,
    },
    Unsupported(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound(path) => write!(f, "file not found: {}", path.display()),
            Self::UnsupportedFormat { detail, .. } => write!(f, "unsupported format: {detail}"),
            Self::OpenFailed { detail, .. } => write!(f, "failed to open media: {detail}"),
            Self::DecodeFailed { track, detail, .. } => {
                write!(f, "decode failed on track {track}: {detail}")
            }
            Self::SeekFailed { detail, .. } => write!(f, "seek failed: {detail}"),
            Self::Unsupported(detail) => write!(f, "unsupported operation: {detail}"),
        }
    }
}

impl std::error::Error for MediaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnsupportedFormat { source, .. }
            | Self::OpenFailed { source, .. }
            | Self::DecodeFailed { source, .. }
            | Self::SeekFailed { source, .. } => as_dyn_source(source),
            Self::FileNotFound(_) | Self::Unsupported(_) => None,
        }
    }
}

impl MediaError {
    pub fn unsupported_format(detail: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            detail: detail.into(),
            source: None,
        }
    }

    pub fn open_failed(detail: impl Into<String>) -> Self {
        Self::OpenFailed {
            detail: detail.into(),
            source: None,
        }
    }

    pub fn decode_failed(track: u32, detail: impl Into<String>) -> Self {
        Self::DecodeFailed {
            track,
            detail: detail.into(),
            source: None,
        }
    }

    pub fn seek_failed(detail: impl Into<String>) -> Self {
        Self::SeekFailed {
            detail: detail.into(),
            source: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FingerprintError {
    InvalidPcm(String),
    EngineFailed {
        detail: String,
        source: Option<ErrorSource>,
    },
}

impl fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPcm(detail) => write!(f, "invalid PCM: {detail}"),
            Self::EngineFailed { detail, .. } => {
                write!(f, "fingerprint engine failed: {detail}")
            }
        }
    }
}

impl std::error::Error for FingerprintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EngineFailed { source, .. } => as_dyn_source(source),
            Self::InvalidPcm(_) => None,
        }
    }
}

impl FingerprintError {
    pub fn engine_failed(detail: impl Into<String>) -> Self {
        Self::EngineFailed {
            detail: detail.into(),
            source: None,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AlignmentError {
    #[error("alignment engine failed: {0}")]
    EngineFailed(String),
}

#[derive(Debug, Clone)]
pub enum ConfigError {
    FileRead {
        path: PathBuf,
        source: Option<ErrorSource>,
    },
    Parse {
        detail: String,
        source: Option<ErrorSource>,
    },
    InvalidValue {
        field: String,
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileRead { path, .. } => {
                write!(f, "failed to read config file: {}", path.display())
            }
            Self::Parse { detail, .. } => write!(f, "failed to parse config: {detail}"),
            Self::InvalidValue { field, reason } => {
                write!(f, "invalid config value for `{field}`: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileRead { source, .. } | Self::Parse { source, .. } => as_dyn_source(source),
            Self::InvalidValue { .. } => None,
        }
    }
}
