use std::process::ExitCode;

use crate::application::error::RepairError;

/// Map a `RepairError` to a process exit code.
///
/// | Code | Meaning |
/// |------|---------|
/// |  0   | Success — analysis complete |
/// |  2   | Config / argument error |
/// |  3   | Domain error (e.g. no audio tracks) |
/// |  4   | Media, I/O, or write error |
/// |  5   | Alignment failure |
/// |  6   | ffmpeg mux failure |
pub fn exit_code_for(error: &RepairError) -> ExitCode {
    match error {
        RepairError::Config(_) => ExitCode::from(2),
        RepairError::Domain(_) => ExitCode::from(3),
        RepairError::Media(_) | RepairError::Io(_) | RepairError::Write(_) => ExitCode::from(4),
        RepairError::Align(_) => ExitCode::from(5),
        RepairError::Mux(_) => ExitCode::from(6),
    }
}
