use std::process::ExitCode;

use crate::application::error::AppError;

pub fn exit_code_for(error: &AppError) -> ExitCode {
    let code = match error {
        AppError::Config(_) => 2,
        AppError::Domain(crate::domain::DomainError::NoAudioTracks) => 3,
        AppError::Domain(_) => 3,
        AppError::Media(_) => 4,
        AppError::Fingerprint(_) => 5,
        AppError::Alignment(_) => 6,
    };
    ExitCode::from(code)
}
