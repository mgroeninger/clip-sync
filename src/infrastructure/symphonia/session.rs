use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::units::TimeBase;
use tracing::debug;

use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{AudioTrack, ClipWindow, MediaSource, MonoPcmClip};
use crate::infrastructure::symphonia::codec_registry::codec_registry;
use crate::infrastructure::symphonia::error_mapping::{
    decode_failed, ensure_regular_file, fail_media, log_media_success, map_decoder_create_error,
};
use crate::infrastructure::symphonia::extract::extract_mono_with_state;
use crate::infrastructure::symphonia::probe::{open_format_reader, probe_media_reusable};

pub struct SymphoniaMediaReader;

impl MediaReader for SymphoniaMediaReader {
    type Session = SymphoniaMediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError> {
        let path = source.path();
        ensure_regular_file(path)?;

        let (tracks, duration, io) = probe_media_reusable(path)?;
        if tracks.is_empty() {
            return Err(fail_media(
                path,
                "open",
                None,
                MediaError::UnsupportedFormat(format!(
                    "no audio tracks in {}",
                    path.display()
                )),
            ));
        }

        if duration.is_zero() {
            return Err(fail_media(
                path,
                "open",
                None,
                MediaError::OpenFailed(format!(
                    "could not determine duration for {}",
                    path.display()
                )),
            ));
        }

        log_media_success(path, "open");
        debug!(
            path = %path.display(),
            track_count = tracks.len(),
            duration_secs = duration.as_secs_f64(),
            "opened media session"
        );

        Ok(SymphoniaMediaSession {
            path: path.to_path_buf(),
            tracks,
            io: RefCell::new(io),
        })
    }
}

pub(crate) struct CachedTrackDecoder {
    pub(crate) decoder: Box<dyn AudioDecoder>,
    pub(crate) time_base: Option<TimeBase>,
}

pub(crate) struct MediaIoState {
    pub(crate) format: Box<dyn symphonia::core::formats::FormatReader>,
    pub(crate) decoders: HashMap<u32, CachedTrackDecoder>,
}

impl MediaIoState {
    pub(crate) fn new(format: Box<dyn symphonia::core::formats::FormatReader>) -> Self {
        Self {
            format,
            decoders: HashMap::new(),
        }
    }

    pub(crate) fn open(path: &Path) -> Result<Self, MediaError> {
        Ok(Self::new(open_format_reader(path)?))
    }
}

pub struct SymphoniaMediaSession {
    path: PathBuf,
    tracks: Vec<AudioTrack>,
    io: RefCell<Option<MediaIoState>>,
}

impl MediaSession for SymphoniaMediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
        Ok(self.tracks.clone())
    }

    fn extract_mono(
        &self,
        track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MonoPcmClip, MediaError> {
        ensure_regular_file(&self.path)?;

        let mut io = self.io.borrow_mut();
        if io.is_none() {
            debug!(
                path = %self.path.display(),
                "reopening format reader for session (probe rewind was unavailable)"
            );
            *io = Some(MediaIoState::open(&self.path)?);
        }

        extract_mono_with_state(
            &self.path,
            io.as_mut().expect("session io initialized"),
            track,
            window,
            progress,
            label,
        )
    }
}

pub(crate) fn ensure_track_decoder(
    path: &Path,
    state: &mut MediaIoState,
    track: &AudioTrack,
) -> Result<(), MediaError> {
    let track_id = track.index;
    if state.decoders.contains_key(&track_id) {
        return Ok(());
    }

    let media_track = state
        .format
        .tracks()
        .iter()
        .find(|candidate| candidate.id == track_id)
        .ok_or_else(|| {
            fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!("track {track_id} not found in {}", path.display()),
                ),
            )
        })?;

    let audio_params = match media_track.codec_params.clone() {
        Some(CodecParameters::Audio(params)) => params,
        _ => {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(track.index, "track has no audio codec parameters"),
            ))
        }
    };

    let decoder = codec_registry()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|error| map_decoder_create_error(path, track.index, error))?;

    state.decoders.insert(
        track_id,
        CachedTrackDecoder {
            decoder,
            time_base: media_track.time_base,
        },
    );

    Ok(())
}

#[cfg(test)]
impl SymphoniaMediaSession {
    pub(crate) fn has_io_state(&self) -> bool {
        self.io.borrow().is_some()
    }

    pub(crate) fn cached_decoder_count(&self) -> usize {
        self.io
            .borrow()
            .as_ref()
            .map(|io| io.decoders.len())
            .unwrap_or(0)
    }
}
