use std::process::ExitCode;

use clip_sync::AppError;

pub fn exit_code_for(error: &AppError) -> ExitCode {
    let code = match error {
        AppError::Config(_) => 2,
        AppError::Domain(_) => 3,
        // Shares the media/IO bucket, matching repair's `Io`/`Write` → 4. A stdout write
        // failure is file I/O; giving it a code of its own would fork the two CLIs for one
        // failure mode without telling a script anything it can act on differently.
        AppError::Media(_) | AppError::Output(_) => 4,
        AppError::Fingerprint(_) => 5,
        AppError::Alignment(_) => 6,
    };
    ExitCode::from(code)
}
