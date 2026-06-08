use std::io;
use std::path::Path;

use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use tracing::{debug, error, warn};

use crate::application::error::MediaError;

pub fn decode_failed(track: u32, detail: impl Into<String>) -> MediaError {
    MediaError::DecodeFailed {
        track,
        detail: detail.into(),
    }
}

pub fn log_media_success(path: &Path, operation: &str) {
    debug!(path = %path.display(), operation, "media operation succeeded");
}

pub fn log_media_failure(path: &Path, operation: &str, track: Option<u32>, error: &MediaError) {
    match error {
        MediaError::FileNotFound(path) => {
            warn!(path = %path.display(), operation, "media file not found");
        }
        MediaError::UnsupportedFormat(detail) => {
            warn!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "unsupported media format or codec"
            );
        }
        MediaError::OpenFailed(detail) => {
            error!(
                path = %path.display(),
                operation,
                detail = %detail,
                "failed to open or probe media"
            );
        }
        MediaError::DecodeFailed { track, detail } => {
            error!(
                path = %path.display(),
                operation,
                track,
                detail = %detail,
                "failed to decode media"
            );
        }
        MediaError::SeekFailed(detail) => {
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
            MediaError::OpenFailed(format!("{} is not a regular file", path.display())),
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
) -> MediaError {
    let media_error = seek_error(path, track, start_secs, error);
    log_media_failure(path, "seek", Some(track), &media_error);
    media_error
}

fn io_error(path: &Path, operation: &str, error: io::Error) -> MediaError {
    match error.kind() {
        io::ErrorKind::NotFound => MediaError::FileNotFound(path.to_path_buf()),
        io::ErrorKind::PermissionDenied => MediaError::OpenFailed(format!(
            "{}: permission denied during {operation}",
            path.display()
        )),
        _ => MediaError::OpenFailed(format!(
            "{}: I/O error during {operation}: {error}",
            path.display()
        )),
    }
}

fn probe_error(path: &Path, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::IoError(err) => io_error(path, "probe", err),
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat(format!(
            "{}: unsupported container or feature: {feature}",
            path.display()
        )),
        SymphoniaError::DecodeError(msg) => MediaError::OpenFailed(format!(
            "{}: malformed media during probe: {msg}",
            path.display()
        )),
        SymphoniaError::SeekError(kind) => MediaError::SeekFailed(format!(
            "{}: seek error during probe: {}",
            path.display(),
            seek_kind_message(kind)
        )),
        SymphoniaError::LimitError(limit) => MediaError::OpenFailed(format!(
            "{}: limit reached during probe: {limit}",
            path.display()
        )),
        SymphoniaError::ResetRequired => MediaError::OpenFailed(format!(
            "{}: unexpected decoder reset during probe",
            path.display()
        )),
        other => MediaError::OpenFailed(format!(
            "{}: probe failed: {other}",
            path.display()
        )),
    }
}

fn decoder_create_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat(format!(
            "{}: unsupported codec on track {track}: {feature}",
            path.display()
        )),
        other => decode_failed(
            track,
            format!("failed to create decoder for track {track}: {other}"),
        ),
    }
}

fn decode_loop_error(path: &Path, track: u32, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::IoError(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
            decode_failed(track, format!("unexpected end of stream: {err}"))
        }
        SymphoniaError::IoError(err) => {
            decode_failed(track, format!("I/O error while decoding: {err}"))
        }
        SymphoniaError::DecodeError(msg) => {
            decode_failed(track, format!("malformed stream while decoding: {msg}"))
        }
        SymphoniaError::SeekError(kind) => MediaError::SeekFailed(format!(
            "{}: seek error on track {track} while decoding: {}",
            path.display(),
            seek_kind_message(kind)
        )),
        SymphoniaError::Unsupported(feature) => MediaError::UnsupportedFormat(format!(
            "{}: unsupported feature on track {track} while decoding: {feature}",
            path.display()
        )),
        SymphoniaError::LimitError(limit) => {
            decode_failed(track, format!("decode limit reached: {limit}"))
        }
        SymphoniaError::ResetRequired => {
            decode_failed(track, "unexpected decoder reset while decoding".to_string())
        }
        other => decode_failed(track, format!("decode failed on track {track}: {other}")),
    }
}

fn seek_error(path: &Path, track: u32, start_secs: f64, error: SymphoniaError) -> MediaError {
    match error {
        SymphoniaError::SeekError(kind) => MediaError::SeekFailed(format!(
            "{}: seek to {start_secs:.3}s on track {track} failed: {}",
            path.display(),
            seek_kind_message(kind)
        )),
        SymphoniaError::Unsupported(feature) => MediaError::SeekFailed(format!(
            "{}: seeking to {start_secs:.3}s on track {track} unsupported: {feature}",
            path.display()
        )),
        other => MediaError::SeekFailed(format!(
            "{}: seek to {start_secs:.3}s on track {track} failed: {other}",
            path.display()
        )),
    }
}

fn seek_kind_message(kind: SeekErrorKind) -> &'static str {
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

    #[test]
    fn maps_not_found_io_error() {
        let path = Path::new("/no/such/file.mkv");
        let error = map_io_error(
            path,
            "open",
            io::Error::new(io::ErrorKind::NotFound, "missing"),
        );
        assert_eq!(error, MediaError::FileNotFound(path.to_path_buf()));
    }

    #[test]
    fn maps_unsupported_probe_error() {
        let path = Path::new("test.xyz");
        let error = map_probe_error(path, SymphoniaError::Unsupported("container"));
        assert!(matches!(error, MediaError::UnsupportedFormat(_)));
    }

    #[test]
    fn maps_unsupported_decoder_error() {
        let path = Path::new("test.mkv");
        let error = map_decoder_create_error(path, 1, SymphoniaError::Unsupported("codec"));
        assert!(matches!(error, MediaError::UnsupportedFormat(_)));
    }

    #[test]
    fn maps_seek_error_during_decode() {
        let path = Path::new("test.wav");
        let error = map_decode_loop_error(path, 1, SymphoniaError::SeekError(SeekErrorKind::Unseekable));
        assert!(matches!(error, MediaError::SeekFailed(_)));
    }

    #[test]
    fn rejects_directory_path() {
        let temp = tempfile::tempdir().unwrap();
        let error = ensure_regular_file(temp.path()).unwrap_err();
        assert!(matches!(error, MediaError::OpenFailed(_)));
    }
}
