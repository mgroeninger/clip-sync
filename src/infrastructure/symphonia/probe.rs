use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::Channels;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use tracing::debug;

use crate::application::error::MediaError;
use crate::domain::AudioTrack;
use crate::infrastructure::symphonia::codec_registry::codec_registry;
use crate::infrastructure::symphonia::duration::{
    duration_from_chapters, format_media_duration, scan_container_audio_duration,
    track_duration_from_track,
};
use crate::infrastructure::symphonia::error_mapping::{
    log_media_success, map_io_error, map_probe_error, map_seek_error,
};
use crate::infrastructure::symphonia::session::MediaIoState;

pub(crate) fn open_format_reader(
    path: &Path,
) -> Result<Box<dyn symphonia::core::formats::FormatReader>, MediaError> {
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let file = File::open(path).map_err(|error| map_io_error(path, "open", error))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| map_probe_error(path, error))
}

/// Probe once and retain the `FormatReader` for later extracts (no second probe on first clip).
pub(crate) fn probe_media_reusable(
    path: &Path,
) -> Result<(Vec<AudioTrack>, Duration, Option<MediaIoState>), MediaError> {
    let mut format = open_format_reader(path)?;
    let (tracks, duration) = probe_from_format(path, format.as_mut())?;
    let io = match rewind_format_reader(path, format.as_mut()) {
        Ok(()) => Some(MediaIoState::new(format)),
        Err(error) => {
            debug!(
                path = %path.display(),
                error = %error,
                "could not rewind format reader after probe; will reopen on extract"
            );
            None
        }
    };
    log_media_success(path, "probe");
    Ok((tracks, duration, io))
}

fn probe_from_format(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
) -> Result<(Vec<AudioTrack>, Duration), MediaError> {
    let media_duration = format_media_duration(format);
    let mut tracks = Vec::new();
    let mut duration = Duration::ZERO;

    for track in format.tracks() {
        if !is_audio_track(track) {
            continue;
        }

        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            continue;
        };

        let track_duration = track_duration_from_track(track).or(media_duration);
        let decodable = is_audio_decodable(params);
        tracks.push(AudioTrack {
            index: track.id,
            codec: codec_name(params.codec),
            channels: channel_count(params.channels.as_ref()),
            sample_rate: params.sample_rate.unwrap_or(0),
            bitrate: None,
            duration: track_duration,
            decodable,
        });

        if let Some(track_duration) = track_duration {
            duration = duration.max(track_duration);
        }
    }

    if let Some(media_duration) = media_duration {
        duration = duration.max(media_duration);
    }

    if duration.is_zero() {
        if let Some(estimated) = duration_from_chapters(format.chapters()) {
            duration = estimated;
        } else {
            duration = scan_container_audio_duration(path, format)?;
        }
    }

    if !duration.is_zero() {
        for track in tracks.iter_mut() {
            if track.duration.is_none() {
                track.duration = Some(duration);
            }
        }
    }

    Ok((tracks, duration))
}

fn rewind_format_reader(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
) -> Result<(), MediaError> {
    let time = Time::ZERO;
    let seek_to = SeekTo::Time {
        time,
        track_id: None,
    };
    format
        .seek(SeekMode::Accurate, seek_to)
        .map_err(|error| map_seek_error(path, 0, 0.0, error))?;
    Ok(())
}

fn is_audio_decodable(params: &symphonia::core::codecs::audio::AudioCodecParameters) -> bool {
    codec_registry()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .is_ok()
}

pub(crate) fn is_audio_track(track: &Track) -> bool {
    track.track_type() == Some(TrackType::Audio)
}

fn channel_count(channels: Option<&Channels>) -> u16 {
    channels.map(|value| value.count() as u16).unwrap_or(0)
}

fn codec_name(codec: AudioCodecId) -> String {
    match codec {
        CODEC_ID_AAC => "aac".into(),
        CODEC_ID_MP3 => "mp3".into(),
        CODEC_ID_FLAC => "flac".into(),
        CODEC_ID_VORBIS => "vorbis".into(),
        CODEC_ID_ALAC => "alac".into(),
        _ => format!("{codec}"),
    }
}
