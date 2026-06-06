use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use symphonia::core::audio::{Channels, GenericAudioBufferRef};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::meta::{ChapterGroup, ChapterGroupItem};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration as MediaDuration, Time, TimeBase, Timestamp};
use tracing::{debug, info};

use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{AudioTrack, ClipWindow, MediaSource, MonoPcmClip};
use crate::infrastructure::symphonia::codec_registry::codec_registry;
use crate::infrastructure::symphonia::error_mapping::{
    decode_failed, ensure_regular_file, fail_media, log_media_success, map_decode_loop_error,
    map_decoder_create_error, map_io_error, map_probe_error, map_seek_error,
};

pub struct SymphoniaMediaReader;

impl MediaReader for SymphoniaMediaReader {
    type Session = SymphoniaMediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError> {
        let path = source.path();
        ensure_regular_file(path)?;

        let (tracks, duration) = probe_media(path)?;
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
        })
    }
}

pub struct SymphoniaMediaSession {
    path: PathBuf,
    tracks: Vec<AudioTrack>,
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
        extract_mono_window(&self.path, track, window, progress, label)
    }
}

fn probe_media(path: &Path) -> Result<(Vec<AudioTrack>, Duration), MediaError> {
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let file = File::open(path).map_err(|error| map_io_error(path, "open", error))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| map_probe_error(path, error))?;

    let media_duration = format_media_duration(format.as_ref());
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
            duration = scan_container_audio_duration(path, format.as_mut())?;
        }
    }

    if !duration.is_zero() {
        for track in tracks.iter_mut() {
            if track.duration.is_none() {
                track.duration = Some(duration);
            }
        }
    }

    log_media_success(path, "probe");
    Ok((tracks, duration))
}

fn is_audio_decodable(params: &symphonia::core::codecs::audio::AudioCodecParameters) -> bool {
    codec_registry()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .is_ok()
}

fn duration_from_chapters(chapters: Option<&ChapterGroup>) -> Option<Duration> {
    let mut max_end = Time::ZERO;

    fn visit(group: &ChapterGroup, max_end: &mut Time) {
        for item in &group.items {
            match item {
                ChapterGroupItem::Chapter(chapter) => {
                    if let Some(end) = chapter.end_time {
                        if end.as_nanos() > max_end.as_nanos() {
                            *max_end = end;
                        }
                    } else if chapter.start_time.as_nanos() > max_end.as_nanos() {
                        *max_end = chapter.start_time;
                    }
                }
                ChapterGroupItem::Group(nested) => visit(nested, max_end),
            }
        }
    }

    visit(chapters?, &mut max_end);
    if max_end.is_zero() {
        None
    } else {
        Some(symphonia_time_to_std(max_end))
    }
}

fn scan_container_audio_duration(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
) -> Result<Duration, MediaError> {
    let audio_time_bases: HashMap<u32, TimeBase> = format
        .tracks()
        .iter()
        .filter(|track| is_audio_track(track))
        .filter_map(|track| track.time_base.map(|time_base| (track.id, time_base)))
        .collect();

    if audio_time_bases.is_empty() {
        return Err(fail_media(
            path,
            "probe",
            None,
            MediaError::OpenFailed(format!(
                "could not determine duration for {}",
                path.display()
            )),
        ));
    }

    info!(
        path = %path.display(),
        "no duration metadata; scanning container packets to estimate length"
    );

    let mut max_duration = Duration::ZERO;
    loop {
        match format.next_packet() {
            Ok(Some(packet)) => {
                if let Some(time_base) = audio_time_bases.get(&packet.track_id) {
                    let end = packet.pts.saturating_add(packet.dur);
                    if let Some(time) = time_base.calc_time(end) {
                        max_duration = max_duration.max(symphonia_time_to_std(time));
                    }
                }
            }
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(error) => {
                let track_id = audio_time_bases.keys().copied().next().unwrap_or(0);
                return Err(map_decode_loop_error(path, track_id, error));
            }
        }
    }

    if max_duration.is_zero() {
        return Err(fail_media(
            path,
            "probe",
            None,
            MediaError::OpenFailed(format!(
                "could not determine duration for {}",
                path.display()
            )),
        ));
    }

    debug!(
        path = %path.display(),
        duration_secs = max_duration.as_secs_f64(),
        "estimated duration from container scan"
    );
    Ok(max_duration)
}

fn extract_mono_window(
    path: &Path,
    track: &AudioTrack,
    window: &ClipWindow,
    progress: &dyn ProgressReporter,
    label: &str,
) -> Result<MonoPcmClip, MediaError> {
    if window.end <= window.start {
        return Err(fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(track.index, "clip window is empty"),
        ));
    }

    ensure_regular_file(path)?;

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let file = File::open(path).map_err(|error| map_io_error(path, "open", error))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| map_probe_error(path, error))?;

    let track_id = track.index;
    let media_track = format
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

    let time_base = media_track.time_base;

    let mut decoder = codec_registry()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|error| map_decoder_create_error(path, track.index, error))?;

    let sample_rate = audio_params
        .sample_rate
        .filter(|rate| *rate > 0)
        .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0));

    seek_to_window_start(path, format.as_mut(), track_id, window.start)?;
    decoder.reset();

    let mut resolved_rate = sample_rate;
    let mut target_samples = resolved_rate.map(|rate| {
        let (start, end) = window_sample_bounds(window, rate);
        end.saturating_sub(start) as usize
    });
    let mut mono_samples = Vec::new();
    if let Some(rate) = resolved_rate {
        mono_samples.reserve(window.sample_count_at(rate));
    }
    if let (Some(rate), Some(expected)) = (resolved_rate, target_samples) {
        debug!(
            path = %path.display(),
            track = track.index,
            window_start_secs = window.start.as_secs_f64(),
            window_end_secs = window.end.as_secs_f64(),
            expected_samples = expected,
            sample_rate = rate,
            "extracting mono clip"
        );
    }

    let mut last_reported = 0_u64;
    let mut finished = false;
    let mut stream_ended = false;

    loop {
        if finished {
            break;
        }
        if let Some(target) = target_samples {
            if mono_samples.len() >= target {
                break;
            }
        }

        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                stream_ended = true;
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(error) => {
                return Err(map_decode_loop_error(path, track.index, error));
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        if let Some(time_base) = time_base {
            if let Some(rate) = resolved_rate {
                let (start_sample, end_sample) = window_sample_bounds(window, rate);
                let packet_start_sample = timestamp_to_sample(packet.pts, time_base, rate);
                let packet_end_sample = timestamp_to_sample(
                    packet.pts.saturating_add(packet.dur),
                    time_base,
                    rate,
                );
                if packet_end_sample <= start_sample {
                    continue;
                }
                if packet_start_sample >= end_sample {
                    break;
                }
            } else if let (Some(packet_start), Some(packet_end)) = (
                timestamp_to_std_duration(packet.pts, time_base),
                timestamp_to_std_duration(packet.pts.saturating_add(packet.dur), time_base),
            ) {
                if packet_end <= window.start {
                    continue;
                }
                if packet_start >= window.end {
                    break;
                }
            }
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                stream_ended = true;
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(error) => {
                return Err(map_decode_loop_error(path, track.index, error));
            }
        };

        if decoded.frames() == 0 {
            continue;
        }

        if resolved_rate.is_none() {
            resolved_rate = Some(decoded.spec().rate());
            let rate = resolved_rate.unwrap_or(0);
            if rate == 0 {
                return Err(fail_media(
                    path,
                    "extract",
                    Some(track.index),
                    decode_failed(track.index, "missing sample rate"),
                ));
            }
            let (start_sample, end_sample) = window_sample_bounds(window, rate);
            let expected = end_sample.saturating_sub(start_sample) as usize;
            if expected == 0 {
                return Err(fail_media(
                    path,
                    "extract",
                    Some(track.index),
                    decode_failed(track.index, "clip window is too short to decode"),
                ));
            }
            target_samples = Some(expected);
            mono_samples.reserve(expected);
            debug!(
                path = %path.display(),
                track = track.index,
                window_start_secs = window.start.as_secs_f64(),
                window_end_secs = window.end.as_secs_f64(),
                expected_samples = expected,
                sample_rate = rate,
                "extracting mono clip"
            );
        }

        let rate = resolved_rate.unwrap_or(0);
        let target = target_samples.unwrap_or(0);
        let (start_sample, end_sample) = window_sample_bounds(window, rate);
        let packet_start_sample = time_base
            .map(|base| timestamp_to_sample(packet.pts, base, rate))
            .unwrap_or(start_sample);
        let trim_start_frames = time_base
            .map(|base| media_duration_to_frames(packet.trim_start, base, rate))
            .unwrap_or(0);

        finished = append_frames_in_window(
            decoded,
            &mut WindowCollectContext {
                packet_start_sample,
                window_start_sample: start_sample,
                window_end_sample: end_sample,
                trim_start_frames,
                mono_samples: &mut mono_samples,
                target_samples: target,
            },
        );

        if mono_samples.len().saturating_sub(last_reported as usize) >= rate as usize / 2 {
            progress.progress(
                label,
                mono_samples.len().min(target) as u64,
                target as u64,
            );
            last_reported = mono_samples.len().min(target) as u64;
        }
    }

    let rate = resolved_rate.ok_or_else(|| {
        fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(track.index, "missing sample rate"),
        )
    })?;
    let target = target_samples.unwrap_or(0);

    mono_samples.truncate(target);
    progress.progress(label, mono_samples.len() as u64, target as u64);

    if mono_samples.is_empty() {
        return Err(fail_media(
            path,
            "extract",
            Some(track.index),
            decode_failed(
                track.index,
                format!(
                    "no audio decoded for window [{:.3}s–{:.3}s)",
                    window.start.as_secs_f64(),
                    window.end.as_secs_f64()
                ),
            ),
        ));
    }

    if mono_samples.len() < target {
        let shortfall = target - mono_samples.len();
        let limit = decode_shortfall_limit(rate, target, stream_ended);
        if shortfall > limit {
            return Err(fail_media(
                path,
                "extract",
                Some(track.index),
                decode_failed(
                    track.index,
                    format!(
                        "partial clip decoded: got {} of {} samples for window [{:.3}s–{:.3}s)",
                        mono_samples.len(),
                        target,
                        window.start.as_secs_f64(),
                        window.end.as_secs_f64()
                    ),
                ),
            ));
        }

        debug!(
            path = %path.display(),
            track = track.index,
            shortfall,
            target,
            stream_ended,
            limit,
            "padding end-of-window decode gap with silence"
        );
        mono_samples.resize(target, 0);
    }

    log_media_success(path, "extract");
    debug!(
        path = %path.display(),
        track = track.index,
        sample_rate = rate,
        samples = mono_samples.len(),
        "extracted mono clip"
    );

    Ok(MonoPcmClip::new(rate, mono_samples))
}

struct WindowCollectContext<'a> {
    packet_start_sample: u64,
    window_start_sample: u64,
    window_end_sample: u64,
    trim_start_frames: u32,
    mono_samples: &'a mut Vec<i16>,
    target_samples: usize,
}

fn append_frames_in_window(
    decoded: GenericAudioBufferRef<'_>,
    ctx: &mut WindowCollectContext<'_>,
) -> bool {
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return false;
    }

    let channel_count = decoded.spec().channels().count().max(1);
    let mut interleaved = Vec::new();
    decoded.copy_to_vec_interleaved(&mut interleaved);

    let trim_start = ctx.trim_start_frames as usize;
    for frame_idx in trim_start..frame_count {
        if ctx.mono_samples.len() >= ctx.target_samples {
            return true;
        }

        let sample_index = ctx.packet_start_sample + (frame_idx - trim_start) as u64;
        if sample_index >= ctx.window_end_sample {
            return true;
        }
        if sample_index < ctx.window_start_sample {
            continue;
        }

        let frame_start = frame_idx * channel_count;
        let frame = &interleaved[frame_start..frame_start + channel_count];
        let mono = if frame.is_empty() {
            0.0
        } else {
            frame.iter().sum::<f32>() / frame.len() as f32
        };
        ctx.mono_samples.push(float_to_i16(mono));
    }

    false
}

fn timestamp_to_std_duration(ts: Timestamp, time_base: TimeBase) -> Option<Duration> {
    time_base.calc_time(ts).map(symphonia_time_to_std)
}

fn timestamp_to_sample(ts: Timestamp, time_base: TimeBase, sample_rate: u32) -> u64 {
    timestamp_to_std_duration(ts, time_base)
        .map(|duration| time_to_sample(duration, sample_rate))
        .unwrap_or(0)
}

fn media_duration_to_frames(dur: MediaDuration, time_base: TimeBase, sample_rate: u32) -> u32 {
    let ts = Timestamp::try_from(dur.get()).unwrap_or(Timestamp::ZERO);
    timestamp_to_sample(ts, time_base, sample_rate).min(u32::MAX as u64) as u32
}

fn time_to_sample(time: Duration, sample_rate: u32) -> u64 {
    (time.as_secs_f64() * f64::from(sample_rate)).floor() as u64
}

fn window_sample_bounds(window: &ClipWindow, sample_rate: u32) -> (u64, u64) {
    (
        time_to_sample(window.start, sample_rate),
        time_to_sample(window.end, sample_rate),
    )
}

fn seek_to_window_start(
    path: &Path,
    format: &mut dyn symphonia::core::formats::FormatReader,
    track_id: u32,
    start: Duration,
) -> Result<(), MediaError> {
    let time = Time::try_new(start.as_secs() as i64, start.subsec_nanos()).ok_or_else(|| {
        MediaError::SeekFailed(format!(
            "{}: seek to {:.3}s on track {track_id} failed: invalid time",
            path.display(),
            start.as_secs_f64()
        ))
    })?;

    let seek_to = SeekTo::Time {
        time,
        track_id: Some(track_id),
    };

    format
        .seek(SeekMode::Accurate, seek_to)
        .map_err(|error| map_seek_error(path, track_id, start.as_secs_f64(), error))?;

    Ok(())
}

fn sample_count_tolerance(sample_rate: u32) -> usize {
    frame_boundary_tolerance(sample_rate)
}

fn frame_boundary_tolerance(sample_rate: u32) -> usize {
    // ~20 ms baseline; allow up to two HE-AAC SBR output frames (2048 samples each) for
    // container duration vs decodable sample boundary mismatch at window edges.
    const HE_AAC_FRAME_SAMPLES: usize = 2048;
    (sample_rate as usize / 50)
        .max(HE_AAC_FRAME_SAMPLES * 2)
        .max(64)
}

fn decode_shortfall_limit(sample_rate: u32, target_samples: usize, stream_eof: bool) -> usize {
    let frame = frame_boundary_tolerance(sample_rate);
    if !stream_eof {
        return frame;
    }

    // Container duration often extends past the last decodable sample; allow a bounded pad at EOF.
    const EOF_MAX_SECS: f64 = 2.0;
    let eof_cap = (f64::from(sample_rate) * EOF_MAX_SECS).ceil() as usize;
    let percent_cap = target_samples / 200; // 0.5%
    frame.max(eof_cap.min(percent_cap))
}

fn track_duration_from_track(track: &Track) -> Option<Duration> {
    let mut candidates = Vec::new();

    if let Some(time_base) = track.time_base {
        if let Some(media_duration) = track.duration {
            if let Some(time) = media_ticks_to_time(media_duration, time_base) {
                candidates.push(symphonia_time_to_std(time));
            }
        }

        if let Some(num_frames) = track.num_frames {
            if let Some(CodecParameters::Audio(params)) = &track.codec_params {
                if let Some(rate) = params.sample_rate.filter(|rate| *rate > 0) {
                    candidates.push(Duration::from_secs_f64(num_frames as f64 / f64::from(rate)));
                }
            } else if let Some(time) = media_ticks_to_time(MediaDuration::new(num_frames), time_base)
            {
                candidates.push(symphonia_time_to_std(time));
            }
        }
    }

    if let Some(CodecParameters::Audio(params)) = &track.codec_params {
        if let (Some(num_frames), Some(rate)) = (track.num_frames, params.sample_rate) {
            if rate > 0 {
                candidates.push(Duration::from_secs_f64(num_frames as f64 / f64::from(rate)));
            }
        }
    }

    candidates.into_iter().min()
}

fn media_ticks_to_time(ticks: MediaDuration, time_base: TimeBase) -> Option<Time> {
    Timestamp::try_from(ticks.get())
        .ok()
        .and_then(|ts| time_base.calc_time(ts))
}

fn format_media_duration(format: &dyn symphonia::core::formats::FormatReader) -> Option<Duration> {
    let info = format.media_info();
    let time_base = info.time_base?;
    let ticks = info.duration?;
    media_ticks_to_time(ticks, time_base).map(symphonia_time_to_std)
}

fn symphonia_time_to_std(time: Time) -> Duration {
    let (seconds, nanos) = time.parts();
    Duration::new(seconds.max(0) as u64, nanos)
}

fn is_audio_track(track: &Track) -> bool {
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

fn float_to_i16(sample: f32) -> i16 {
    let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i32;
    scaled.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use hound::{SampleFormat, WavSpec, WavWriter};
    use symphonia::core::audio::{layouts, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef};

    use super::*;
    use crate::application::ports::ProgressReporter;
    use crate::domain::ClipLabel;

    struct NoopProgress;

    impl ProgressReporter for NoopProgress {
        fn phase(&self, _message: &str) {}
        fn progress(&self, _label: &str, _current: u64, _total: u64) {}
    }

    fn write_test_wav(path: &Path, sample_rate: u32, seconds: u32) {
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        let total_samples = sample_rate as u64 * seconds as u64;

        for index in 0..total_samples {
            let t = index as f32 / sample_rate as f32;
            let sample = (TAU * 440.0 * t).sin();
            let amplitude = (sample * i16::MAX as f32) as i16;
            writer.write_sample(amplitude).unwrap();
            writer.write_sample(amplitude).unwrap();
        }

        writer.finalize().unwrap();
    }

    #[test]
    fn float_to_i16_clamps() {
        assert_eq!(float_to_i16(2.0), i16::MAX);
        assert_eq!(float_to_i16(-2.0), -i16::MAX);
    }

    #[test]
    fn window_sample_bounds_rounds_up() {
        let window = ClipWindow::new(
            Duration::from_millis(500),
            Duration::from_millis(1500),
            ClipLabel::Start,
        );
        let (start, end) = window_sample_bounds(&window, 44_100);
        assert_eq!(end.saturating_sub(start) as usize, 44_100);
        assert_eq!(window.sample_count_at(44_100), 44_100);
    }

    #[test]
    fn sample_count_tolerance_allows_he_aac_end_boundary_gap() {
        assert!(sample_count_tolerance(48_000) >= 4_096);
        assert!(sample_count_tolerance(48_000) >= 3_136);
    }

    #[test]
    fn decode_shortfall_limit_allows_eof_tail_on_long_clips() {
        let target = 43_200_000_usize;
        let limit = decode_shortfall_limit(48_000, target, true);
        assert!(limit >= 76_304, "limit was {limit}");
        assert!(limit <= 96_000, "limit was {limit}");
    }

    #[test]
    fn decode_shortfall_limit_stays_strict_without_eof() {
        assert_eq!(decode_shortfall_limit(48_000, 43_200_000, false), 4_096);
    }

    #[test]
    fn append_frames_in_window_downmixes_stereo() {
        let spec = AudioSpec::new(44_100, layouts::CHANNEL_LAYOUT_STEREO);
        let mut buffer = AudioBuffer::<f32>::new(spec, 2);
        buffer.render_silence(Some(2));
        buffer.plane_mut(0).unwrap()[0] = 1.0;
        buffer.plane_mut(1).unwrap()[0] = -1.0;
        buffer.plane_mut(0).unwrap()[1] = 0.5;
        buffer.plane_mut(1).unwrap()[1] = 0.5;

        let mut mono = Vec::new();
        append_frames_in_window(
            GenericAudioBufferRef::F32(&buffer),
            &mut WindowCollectContext {
                packet_start_sample: 0,
                window_start_sample: 0,
                window_end_sample: 2,
                trim_start_frames: 0,
                mono_samples: &mut mono,
                target_samples: 2,
            },
        );

        assert_eq!(mono.len(), 2);
        assert_eq!(mono[0], 0);
        assert!(mono[1] > 0);
    }

    #[test]
    fn append_frames_in_window_skips_before_window_start() {
        let spec = AudioSpec::new(44_100, layouts::CHANNEL_LAYOUT_MONO);
        let mut buffer = AudioBuffer::<f32>::new(spec, 2);
        buffer.render_silence(Some(2));
        buffer.plane_mut(0).unwrap()[0] = 1.0;
        buffer.plane_mut(0).unwrap()[1] = 0.25;

        let mut mono = Vec::new();
        append_frames_in_window(
            GenericAudioBufferRef::F32(&buffer),
            &mut WindowCollectContext {
                packet_start_sample: 9,
                window_start_sample: 10,
                window_end_sample: 20,
                trim_start_frames: 0,
                mono_samples: &mut mono,
                target_samples: 1,
            },
        );

        assert_eq!(mono.len(), 1);
        assert!(mono[0] > 0);
    }

    #[test]
    fn probe_and_extract_wav() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone.wav");
        write_test_wav(&path, 44_100, 3);

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].sample_rate, 44_100);
        assert_eq!(tracks[0].channels, 2);
        assert!(tracks[0].duration.unwrap().as_secs() >= 2);
        assert!(duration.as_secs() >= 2);

        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip = extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "test")
            .unwrap();

        assert_eq!(clip.sample_rate, 44_100);
        assert_eq!(clip.samples.len(), 44_100);
    }

    #[test]
    fn media_reader_open_lists_tracks() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone.wav");
        write_test_wav(&path, 44_100, 2);

        let reader = SymphoniaMediaReader;
        let session = reader.open(&MediaSource::new(&path)).unwrap();
        let tracks = session.list_tracks().unwrap();
        assert_eq!(tracks.len(), 1);
        assert!(tracks[0].duration.unwrap().as_secs() >= 1);
    }

    #[test]
    fn open_rejects_missing_file() {
        let reader = SymphoniaMediaReader;
        match reader.open(&MediaSource::new("definitely-missing-file.wav")) {
            Err(MediaError::FileNotFound(_)) => {}
            Ok(_) => panic!("expected FileNotFound"),
            Err(other) => panic!("expected FileNotFound, got {other}"),
        }
    }

    #[test]
    fn extract_window_skips_pre_window_audio() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("split_tone.wav");
        write_split_tone_wav(&path, 44_100, 2);

        let (tracks, _) = probe_media(&path).unwrap();
        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip = extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "test")
            .unwrap();

        assert!((clip.samples.len() as i64 - 44_100).abs() <= sample_count_tolerance(44_100) as i64);
        let peak = clip
            .samples
            .iter()
            .map(|sample| sample.abs())
            .max()
            .unwrap_or(0);
        assert!(peak > 1_000, "expected high-amplitude samples in second half window");
    }

    fn write_split_tone_wav(path: &Path, sample_rate: u32, seconds: u32) {
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        let total_samples = sample_rate as u64 * seconds as u64;

        for index in 0..total_samples {
            let sample = if index < sample_rate as u64 {
                0_i16
            } else {
                i16::MAX / 2
            };
            writer.write_sample(sample).unwrap();
        }

        writer.finalize().unwrap();
    }

    #[cfg(feature = "ffmpeg-tests")]
    mod ffmpeg {
        use super::*;

        pub(super) fn ffmpeg_available() -> bool {
            std::process::Command::new("ffmpeg")
                .arg("-version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        pub(super) fn write_container_fixture(
            path: &Path,
            format_args: &[&str],
            audio_codec_args: &[&str],
        ) -> bool {
            if !ffmpeg_available() {
                return false;
            }

            std::process::Command::new("ffmpeg")
                .arg("-y")
                .arg("-loglevel")
                .arg("error")
                .arg("-f")
                .arg("lavfi")
                .arg("-i")
                .arg("sine=frequency=440:duration=3")
                .args(format_args)
                .args(audio_codec_args)
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        }

        #[cfg(feature = "he-aac")]
        pub(super) fn write_he_aac_mp4_fixture(path: &Path) -> bool {
            if !ffmpeg_available() {
                return false;
            }

            let attempts: &[&[&str]] = &[
                &["-c:a", "libfdk_aac", "-profile:a", "aac_he", "-b:a", "64k"],
                &["-c:a", "aac", "-profile:a", "aac_he", "-b:a", "64k"],
            ];

            for audio_codec_args in attempts {
                if write_container_fixture(path, &["-f", "mp4"], audio_codec_args) {
                    return true;
                }
            }

            false
        }

        #[cfg(feature = "he-aac")]
        pub(super) fn write_he_aac_surround_mp4_fixture(path: &Path) -> bool {
            if !ffmpeg_available() {
                return false;
            }

            let attempts: &[&[&str]] = &[
                &[
                    "-c:a",
                    "libfdk_aac",
                    "-profile:a",
                    "aac_he",
                    "-ac",
                    "6",
                    "-b:a",
                    "128k",
                ],
                &["-c:a", "aac", "-profile:a", "aac_he", "-ac", "6", "-b:a", "128k"],
            ];

            for audio_codec_args in attempts {
                if write_container_fixture(path, &["-f", "mp4"], audio_codec_args) {
                    return true;
                }
            }

            false
        }
    }

    #[cfg(feature = "ffmpeg-tests")]
    #[test]
    fn probe_and_extract_mkv_container() {
        use ffmpeg::write_container_fixture;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone.mkv");
        if !write_container_fixture(&path, &["-f", "matroska"], &["-c:a", "flac"]) {
            eprintln!("skipping MKV test: ffmpeg unavailable or encode failed");
            return;
        }

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
        assert!(tracks[0].decodable);
        assert!(tracks[0].duration.unwrap().as_secs() >= 2);
        assert!(duration.as_secs() >= 2);

        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip = extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "mkv")
            .unwrap();
        let expected = tracks[0].sample_rate as usize;
        assert!((clip.samples.len() as i64 - expected as i64).abs() <= sample_count_tolerance(tracks[0].sample_rate) as i64);
    }

    #[cfg(feature = "ffmpeg-tests")]
    #[test]
    fn probe_and_extract_mp4_container() {
        use ffmpeg::write_container_fixture;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone.mp4");
        if !write_container_fixture(
            &path,
            &["-f", "mp4"],
            &["-c:a", "aac", "-b:a", "128k"],
        ) {
            eprintln!("skipping MP4 test: ffmpeg unavailable or encode failed");
            return;
        }

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
        assert!(tracks[0].decodable);
        assert!(tracks[0].duration.unwrap().as_secs() >= 2);
        assert!(duration.as_secs() >= 2);

        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip = extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "mp4")
            .unwrap();
        let expected = tracks[0].sample_rate as usize;
        assert!((clip.samples.len() as i64 - expected as i64).abs() <= sample_count_tolerance(tracks[0].sample_rate) as i64);
    }

    #[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
    #[test]
    fn probe_and_extract_he_aac_mp4_container() {
        use ffmpeg::write_he_aac_mp4_fixture;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone-he-aac.mp4");
        if !write_he_aac_mp4_fixture(&path) {
            eprintln!("skipping HE-AAC MP4 test: ffmpeg unavailable or HE-AAC encode failed");
            return;
        }

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].codec, "aac");
        assert!(
            tracks[0].decodable,
            "HE-AAC track should be decodable when the he-aac feature is enabled"
        );
        assert!(tracks[0].duration.unwrap().as_secs() >= 2);
        assert!(duration.as_secs() >= 2);

        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip = extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "he-aac mp4")
            .unwrap();
        let expected = tracks[0].sample_rate as usize;
        assert!(
            (clip.samples.len() as i64 - expected as i64).abs()
                <= sample_count_tolerance(tracks[0].sample_rate) as i64
        );
        let peak = clip.samples.iter().map(|sample| sample.abs()).max().unwrap_or(0);
        assert!(
            peak > 100,
            "expected non-silent PCM from HE-AAC decode, peak={peak}"
        );
    }

    #[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
    #[test]
    fn probe_and_extract_he_aac_surround_mp4_container() {
        use ffmpeg::write_he_aac_surround_mp4_fixture;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone-he-aac-51.mp4");
        if !write_he_aac_surround_mp4_fixture(&path) {
            eprintln!(
                "skipping HE-AAC surround MP4 test: ffmpeg unavailable or HE-AAC 5.1 encode failed"
            );
            return;
        }

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].codec, "aac");
        assert!(
            tracks[0].channels >= 6,
            "expected surround channel layout, got {} channels",
            tracks[0].channels
        );
        assert!(
            tracks[0].decodable,
            "HE-AAC 5.1 track should be decodable when the he-aac feature is enabled"
        );
        assert!(duration.as_secs() >= 2);

        let window = ClipWindow::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            ClipLabel::Interior,
        );
        let clip =
            extract_mono_window(&path, &tracks[0], &window, &NoopProgress, "he-aac surround mp4")
                .unwrap();
        let expected = tracks[0].sample_rate as usize;
        assert!(
            (clip.samples.len() as i64 - expected as i64).abs()
                <= sample_count_tolerance(tracks[0].sample_rate) as i64
        );
        let peak = clip.samples.iter().map(|sample| sample.abs()).max().unwrap_or(0);
        assert!(
            peak > 100,
            "expected non-silent downmixed PCM from HE-AAC 5.1 decode, peak={peak}"
        );
    }

    #[test]
    fn open_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();
        let reader = SymphoniaMediaReader;
        match reader.open(&MediaSource::new(temp.path())) {
            Err(MediaError::OpenFailed(_)) => {}
            Ok(_) => panic!("expected OpenFailed"),
            Err(other) => panic!("expected OpenFailed, got {other}"),
        }
    }
}
