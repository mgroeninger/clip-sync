use std::io;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use tracing::{debug, error, warn};

use crate::application::error::MediaError;

/// Seek/decode positions within this many seconds of reported track duration are treated as
/// expected tail-boundary conditions (MKV/AAC metadata vs decoder seek range).
pub(crate) const NEAR_TRACK_END_TOLERANCE_SECS: f64 = 2.0;

pub fn decode_failed(track: u32, detail: impl Into<String>) -> MediaError {
    MediaError::decode_failed(track, detail)
}

pub fn log_media_success(path: &Path, operation: &str) {
    debug!(path = %path.display(), operation, "media operation succeeded");
}

pub fn log_media_failure(path: &Path, operation: &str, track: Option<u32>, error: &MediaError) {
    match error {
        MediaError::FileNotFound(path) => {
            warn!(path = %path.display(), operation, "media file not found");
        }
        MediaError::UnsupportedFormat { detail, .. } => {
            warn!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "unsupported media format or codec"
            );
        }
        MediaError::OpenFailed { detail, .. } => {
            error!(
                path = %path.display(),
                operation,
                detail = %detail,
                "failed to open or probe media"
            );
        }
        MediaError::DecodeFailed { track, detail, .. } => {
            error!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "failed to decode media"
            );
        }
        MediaError::SeekFailed { detail, .. } => {
            error!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "failed to seek in media"
            );
        }
        MediaError::Unsupported(detail) => {
            warn!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "unsupported media operation"
            );
        }
    }
}

pub fn fail_media(
    path: &Path,
    operation: &str,
    track: Option<u32>,
    error: MediaError,
) -> MediaError {
    log_media_failure(path, operation, track, &error);
    error
}

/// Like [`fail_media`] but downgrades expected partial-decode situations from ERROR to WARN.
pub fn warn_partial_decode(
    path: &Path,
    operation: &str,
    track: Option<u32>,
    error: MediaError,
    window_end_secs: f64,
    track_duration_secs: Option<f64>,
) -> MediaError {
    match &error {
        MediaError::DecodeFailed {
            track: t, detail, ..
        } => {
            let near_track_end = track_duration_secs
                .map(|duration| window_end_secs >= duration - NEAR_TRACK_END_TOLERANCE_SECS)
                .unwrap_or(false);
            let reason = if near_track_end {
                "near track end (metadata/decoder boundary)"
            } else {
                "after seek (timestamp/sample boundary mismatch)"
            };
            warn!(
                path = %path.display(),
                operation,
                track = t,
                detail = %detail,
                reason,
                "partial media decode"
            );
        }
        _ => log_media_failure(path, operation, track, &error),
    }
    error
}

pub fn map_io_error(path: &Path, operation: &str, error: io::Error) -> MediaError {
    let media_error = io_error(path, operation, error);
    log_media_failure(path, operation, None, &media_error);
    media_error
}

pub fn ensure_regular_file(path: &Path) -> Result<(), MediaError> {
    let metadata = std::fs::metadata(path).map_err(|error| map_io_error(path, "stat", error))?;
    if !metadata.is_file() {
        return Err(fail_media(
            path,
            "open",
            None,
            MediaError::open_failed(format!("{} is not a regular file", path.display())),
        ));
    }
    Ok(())
}

pub fn map_probe_error(path: &Path, error: SymphoniaError) -> MediaError {
    let media_error = probe_error(path, error);
    log_media_failure(path, "probe", None, &media_error);
    media_error
}

pub fn map_decoder_create_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    let media_error = decoder_create_error(path, track, error);
    log_media_failure(path, "create_decoder", Some(track), &media_error);
    media_error
}

pub fn map_decode_loop_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    let media_error = decode_loop_error(path, track, error);
    log_media_failure(path, "decode", Some(track), &media_error);
    media_error
}

pub fn map_seek_error(
    path: &Path,
    track: u32,
    start_secs: f64,
    error: SymphoniaError,
    track_duration_secs: Option<f64>,
) -> MediaError {
    let expected_tail_seek = is_expected_tail_seek(&error, start_secs, track_duration_secs);
    let media_error = seek_error(path, track, start_secs, error);
    if expected_tail_seek {
        if let MediaError::SeekFailed { detail, .. } = &media_error {
            warn!(
                path = %path.display(),
                operation = "seek",
                track,
                detail = %detail,
                "seek near track end (metadata/decoder boundary)"
            );
        }
    } else {
        log_media_failure(path, "seek", Some(track), &media_error);
    }
    media_error
}

fn is_expected_tail_seek(
    error: &SymphoniaError,
    start_secs: f64,
    track_duration_secs: Option<f64>,
) -> bool {
    let Some(duration) = track_duration_secs else {
        return false;
    };
    matches!(error, SymphoniaError::SeekError(SeekErrorKind::OutOfRange))
        && start_secs >= duration - NEAR_TRACK_END_TOLERANCE_SECS
}

fn io_error(path: &Path, operation: &str, error: io::Error) -> MediaError {
    match error.kind() {
        io::ErrorKind::NotFound => MediaError::FileNotFound(path.to_path_buf()),
        io::ErrorKind::PermissionDenied => MediaError::OpenFailed {
            detail: format!("{}: permission denied during {operation}", path.display()),
            source: Some(Arc::new(error)),
        },
        _ => MediaError::OpenFailed {
            detail: format!("{}: I/O error during {operation}: {error}", path.display()),
            source: Some(Arc::new(error)),
        },
    }
}

fn probe_error(path: &Path, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::IoError(err) => io_error(path, "probe", err),
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat {
            detail: format!(
                "{}: unsupported container or feature: {feature}",
                path.display()
            ),
            source: Some(Arc::new(SymphoniaError::Unsupported(feature))),
        },
        SymphoniaError::DecodeError(msg) => MediaError::OpenFailed {
            detail: format!("{}: malformed media during probe: {msg}", path.display()),
            source: Some(Arc::new(SymphoniaError::DecodeError(msg))),
        },
        SymphoniaError::SeekError(kind) => MediaError::SeekFailed {
            detail: format!(
                "{}: seek error during probe: {}",
                path.display(),
                seek_kind_message(&kind)
            ),
            source: Some(Arc::new(SymphoniaError::SeekError(kind))),
        },
        SymphoniaError::LimitError(limit) => MediaError::OpenFailed {
            detail: format!("{}: limit reached during probe: {limit}", path.display()),
            source: Some(Arc::new(SymphoniaError::LimitError(limit))),
        },
        SymphoniaError::ResetRequired => MediaError::OpenFailed {
            detail: format!("{}: unexpected decoder reset during probe", path.display()),
            source: Some(Arc::new(SymphoniaError::ResetRequired)),
        },
        other => MediaError::OpenFailed {
            detail: format!("{}: probe failed: {other}", path.display()),
            source: Some(Arc::new(other)),
        },
    }
}

fn decoder_create_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat {
            detail: format!(
                "{}: unsupported codec on track {track}: {feature}",
                path.display()
            ),
            source: Some(Arc::new(SymphoniaError::Unsupported(feature))),
        },
        other => MediaError::DecodeFailed {
            track,
            detail: format!("failed to create decoder for track {track}: {other}"),
            source: Some(Arc::new(other)),
        },
    }
}

fn decode_loop_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::IoError(err) => MediaError::DecodeFailed {
            track,
            detail: if err.kind() == io::ErrorKind::UnexpectedEof {
                format!("unexpected end of stream: {err}")
            } else {
                format!("I/O error while decoding: {err}")
            },
            source: Some(Arc::new(err)),
        },
        SymphoniaError::DecodeError(msg) => MediaError::DecodeFailed {
            track,
            detail: format!("malformed stream while decoding: {msg}"),
            source: Some(Arc::new(SymphoniaError::DecodeError(msg))),
        },
        SymphoniaError::SeekError(kind) => MediaError::SeekFailed {
            detail: format!(
                "{}: seek error on track {track} while decoding: {}",
                path.display(),
                seek_kind_message(&kind)
            ),
            source: Some(Arc::new(SymphoniaError::SeekError(kind))),
        },
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat {
            detail: format!(
                "{}: unsupported feature on track {track} while decoding: {feature}",
                path.display()
            ),
            source: Some(Arc::new(SymphoniaError::Unsupported(feature))),
        },
        SymphoniaError::LimitError(limit) => MediaError::DecodeFailed {
            track,
            detail: format!("decode limit reached: {limit}"),
            source: Some(Arc::new(SymphoniaError::LimitError(limit))),
        },
        SymphoniaError::ResetRequired => MediaError::DecodeFailed {
            track,
            detail: "unexpected decoder reset while decoding".to_string(),
            source: Some(Arc::new(SymphoniaError::ResetRequired)),
        },
        other => MediaError::DecodeFailed {
            track,
            detail: format!("decode failed on track {track}: {other}"),
            source: Some(Arc::new(other)),
        },
    }
}

fn seek_error(path: &Path, track: u32, start_secs: f64, error: SymphoniaError) -> MediaError {
    let detail = match &error {
        SymphoniaError::SeekError(kind) => format!(
            "{}: seek to {start_secs:.3}s on track {track} failed: {}",
            path.display(),
            seek_kind_message(kind)
        ),
        SymphoniaError::Unsupported(feature) => format!(
            "{}: seeking to {start_secs:.3}s on track {track} unsupported: {feature}",
            path.display()
        ),
        other => format!(
            "{}: seek to {start_secs:.3}s on track {track} failed: {other}",
            path.display()
        ),
    };
    MediaError::SeekFailed {
        detail,
        source: Some(Arc::new(error)),
    }
}

fn seek_kind_message(kind: &SeekErrorKind) -> &'static str {
    match kind {
        SeekErrorKind::Unseekable => "stream is not seekable",
        SeekErrorKind::ForwardOnly => "stream can only be seeked forward",
        SeekErrorKind::OutOfRange => "requested seek timestamp is out-of-range for stream",
        SeekErrorKind::InvalidTrack => "invalid track id",
        _ => "seek error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk the `source()` chain and return true when some level downcasts to `io::Error`.
    fn chain_reaches_io_error(error: &(dyn std::error::Error + 'static)) -> bool {
        let mut current = error.source();
        while let Some(cause) = current {
            if cause.is::<io::Error>() {
                return true;
            }
            current = cause.source();
        }
        false
    }

    #[test]
    fn maps_not_found_io_error() {
        let path = Path::new("/no/such/file.mkv");
        let error = map_io_error(
            path,
            "open",
            io::Error::new(io::ErrorKind::NotFound, "missing"),
        );
        assert!(matches!(error, MediaError::FileNotFound(p) if p == path));
    }

    #[test]
    fn maps_unsupported_probe_error() {
        let path = Path::new("test.xyz");
        let error = map_probe_error(path, SymphoniaError::Unsupported("container"));
        assert!(matches!(error, MediaError::UnsupportedFormat { .. }));
    }

    #[test]
    fn maps_unsupported_decoder_error() {
        let path = Path::new("test.mkv");
        let error = map_decoder_create_error(path, 1, SymphoniaError::Unsupported("codec"));
        assert!(matches!(error, MediaError::UnsupportedFormat { .. }));
    }

    #[test]
    fn maps_seek_error_during_decode() {
        let path = Path::new("test.wav");
        let error = map_decode_loop_error(
            path,
            1,
            SymphoniaError::SeekError(SeekErrorKind::Unseekable),
        );
        assert!(matches!(error, MediaError::SeekFailed { .. }));
    }

    #[test]
    fn rejects_directory_path() {
        let temp = tempfile::tempdir().unwrap();
        let error = ensure_regular_file(temp.path()).unwrap_err();
        assert!(matches!(error, MediaError::OpenFailed { .. }));
    }

    #[test]
    fn probe_io_failure_source_chain_reaches_io_error() {
        let path = Path::new("test.mkv");
        let media_error = map_probe_error(
            path,
            SymphoniaError::IoError(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        );
        let display = media_error.to_string();
        assert!(
            display.contains("permission denied during probe"),
            "display text must stay stable: {display}"
        );
        // `AppError` is `#[error(transparent)]`, so its source() defers to MediaError's.
        let app_error = crate::application::error::AppError::Media(media_error);
        assert!(
            chain_reaches_io_error(&app_error),
            "expected io::Error reachable via source() from AppError"
        );
    }

    #[test]
    fn decode_io_failure_source_chain_reaches_io_error() {
        let path = Path::new("test.mkv");
        let media_error = map_decode_loop_error(
            path,
            2,
            SymphoniaError::IoError(io::Error::new(io::ErrorKind::UnexpectedEof, "eof")),
        );
        assert!(matches!(
            media_error,
            MediaError::DecodeFailed { track: 2, .. }
        ));
        let app_error = crate::application::error::AppError::Media(media_error);
        assert!(
            chain_reaches_io_error(&app_error),
            "expected io::Error reachable via source() from AppError"
        );
    }

    #[test]
    fn probe_symphonia_failure_attaches_source() {
        use std::error::Error as _;
        let error = map_probe_error(Path::new("x.mkv"), SymphoniaError::Unsupported("container"));
        let source = error.source().expect("symphonia error attached as source");
        assert!(source.is::<SymphoniaError>());
    }

    #[test]
    fn tail_out_of_range_seek_maps_to_seek_failed() {
        let path = Path::new("test.mkv");
        let error = map_seek_error(
            path,
            2,
            600.0,
            SymphoniaError::SeekError(SeekErrorKind::OutOfRange),
            Some(600.064),
        );
        assert!(matches!(error, MediaError::SeekFailed { .. }));
    }

    #[test]
    fn mid_file_out_of_range_seek_still_maps_to_seek_failed() {
        let path = Path::new("test.mkv");
        let error = map_seek_error(
            path,
            2,
            120.0,
            SymphoniaError::SeekError(SeekErrorKind::OutOfRange),
            Some(600.064),
        );
        assert!(matches!(error, MediaError::SeekFailed { .. }));
    }
}
