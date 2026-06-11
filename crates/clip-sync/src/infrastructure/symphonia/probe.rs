use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_AC3, CODEC_ID_ALAC, CODEC_ID_EAC3, CODEC_ID_FLAC, CODEC_ID_MP3,
    CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters, AudioDecoderOptions};
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
            channels: channel_count(params),
            sample_rate: params.sample_rate.unwrap_or(0),

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
        .map_err(|error| map_seek_error(path, 0, 0.0, error, None))?;
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

fn channel_count(params: &AudioCodecParameters) -> u16 {
    if let Some(ch) = params.channels.as_ref() {
        return ch.count() as u16;
    }
    // Symphonia's isomp4 dac3/dec3 atom handler sets codec_id and extra_data but
    // does NOT populate channels. Derive from the codec-specific box payload.
    if params.codec == CODEC_ID_AC3 {
        if let Some(extra) = params.extra_data.as_deref() {
            return ac3_channels_from_dac3(extra);
        }
    }
    if params.codec == CODEC_ID_EAC3 {
        if let Some(extra) = params.extra_data.as_deref() {
            return ac3_channels_from_dec3(extra);
        }
    }
    0
}

/// Decode channel count from the `dac3` box payload (AC-3 in MP4).
///
/// Bit layout (24 bits, MSB first):
///   [23:22] fscod  [21:17] bsid  [16:14] bsmod
///   [13:11] acmod (byte[1] bits 5-3)
///   [10]    lfeon (byte[1] bit 2)
fn ac3_channels_from_dac3(extra: &[u8]) -> u16 {
    if extra.len() < 3 {
        return 0;
    }
    let acmod = (extra[1] >> 3) & 0x7;
    let lfeon = (extra[1] >> 2) & 0x1;
    let base: u16 = match acmod {
        0 => 2, 1 => 1, 2 => 2, 3 => 3,
        4 => 3, 5 => 4, 6 => 4, 7 => 5,
        _ => 0,
    };
    base + u16::from(lfeon)
}

/// Decode channel count from the `dec3` box payload (E-AC-3 in MP4).
///
/// Bit layout for the first independent substream (byte[3]):
///   asvc(1) | bsmod(3) | acmod(3) | lfeon(1)
fn ac3_channels_from_dec3(extra: &[u8]) -> u16 {
    if extra.len() < 4 {
        return 0;
    }
    let acmod = (extra[3] >> 1) & 0x7;
    let lfeon = extra[3] & 0x1;
    let base: u16 = match acmod {
        0 => 2, 1 => 1, 2 => 2, 3 => 3,
        4 => 3, 5 => 4, 6 => 4, 7 => 5,
        _ => 0,
    };
    base + u16::from(lfeon)
}

fn codec_name(codec: AudioCodecId) -> String {
    match codec {
        CODEC_ID_AAC => "aac".into(),
        CODEC_ID_AC3 => "ac3".into(),
        CODEC_ID_EAC3 => "eac3".into(),
        CODEC_ID_MP3 => "mp3".into(),
        CODEC_ID_FLAC => "flac".into(),
        CODEC_ID_VORBIS => "vorbis".into(),
        CODEC_ID_ALAC => "alac".into(),
        _ => format!("{codec}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::codecs::audio::well_known::CODEC_ID_AC3;
    #[cfg(feature = "ac3")]
    use symphonia::core::codecs::audio::well_known::CODEC_ID_EAC3;

    fn ac3_params(codec: AudioCodecId) -> AudioCodecParameters {
        AudioCodecParameters {
            codec,
            sample_rate: Some(48_000),
            ..Default::default()
        }
    }

    #[cfg(feature = "ac3")]
    #[test]
    fn ac3_params_are_decodable_when_feature_enabled() {
        assert!(
            is_audio_decodable(&ac3_params(CODEC_ID_AC3)),
            "AC-3 params should be decodable when the ac3 feature is enabled"
        );
    }

    #[cfg(feature = "ac3")]
    #[test]
    fn eac3_params_are_decodable_when_feature_enabled() {
        assert!(
            is_audio_decodable(&ac3_params(CODEC_ID_EAC3)),
            "E-AC-3 params should be decodable when the ac3 feature is enabled"
        );
    }

    #[cfg(not(feature = "ac3"))]
    #[test]
    fn ac3_params_are_not_decodable_without_feature() {
        assert!(
            !is_audio_decodable(&ac3_params(CODEC_ID_AC3)),
            "AC-3 params should not be decodable when the ac3 feature is disabled"
        );
    }

    #[test]
    fn dac3_channel_count_5_1() {
        // acmod=7 (5 channels) + lfeon=1 → 6 channels
        // byte[1] = acmod(7)<<3 | lfeon(1)<<2 | ... = 0b00111100 = 0x3C
        let extra = [0x00u8, 0x3C, 0x00];
        assert_eq!(ac3_channels_from_dac3(&extra), 6);
    }

    #[test]
    fn dac3_channel_count_stereo() {
        // acmod=2 (stereo) + lfeon=0 → 2 channels
        // byte[1] = acmod(2)<<3 | lfeon(0)<<2 = 0b00010000 = 0x10
        let extra = [0x00u8, 0x10, 0x00];
        assert_eq!(ac3_channels_from_dac3(&extra), 2);
    }

    #[test]
    fn dac3_channel_count_too_short_returns_zero() {
        assert_eq!(ac3_channels_from_dac3(&[0x00, 0x3C]), 0);
    }
}
