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

    /// Post-scan gap selection (`--only-gaps` / `--skip-gaps`) failed validation. Exit 2; under
    /// `--format json`, stdout must stay empty (no success-shaped scan document).
    #[error("gap selection: {0}")]
    GapSelection(String),

    /// Output or report I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Error writing the patched audio output file.
    #[error("write error: {0}")]
    Write(std::io::Error),

    /// ffmpeg mux failure (missing binary, non-zero exit, or stderr message).
    #[error("mux error: {0}")]
    Mux(String),
}
