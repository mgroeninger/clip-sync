use clip_sync::{AppError, DomainError, MediaError};

#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    /// Failure in the alignment sub-flow.
    #[error("alignment failed: {0}")]
    Align(#[from] AppError),

    /// Domain policy failure (e.g. no audio tracks in video A).
    #[error("{0}")]
    Domain(#[from] DomainError),

    /// Media I/O failure during the gap scan phase.
    #[error("{0}")]
    Media(MediaError),

    /// Configuration or argument error.
    #[error("config error: {0}")]
    Config(String),

    /// Output or report I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
