use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use symphonia::core::audio::{AudioBufferRef, Channels, SampleBuffer};
use symphonia::core::codecs::{
    CodecType, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3,
    CODEC_TYPE_NULL, CODEC_TYPE_VORBIS,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};
use tracing::debug;

use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{AudioTrack, ClipWindow, MediaSource, MonoPcmClip};
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

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|error| map_probe_error(path, error))?;

    let mut tracks = Vec::new();
    let mut duration = Duration::ZERO;

    for track in probed.format.tracks() {
        if !is_audio_track(track) {
            continue;
        }

        let params = &track.codec_params;
        let track_duration = track_duration_from_params(params);
        tracks.push(AudioTrack {
            index: track.id,
            codec: codec_name(params.codec),
            channels: channel_count(params.channels),
            sample_rate: params.sample_rate.unwrap_or(0),
            bitrate: None,
            duration: track_duration,
        });

        if let Some(track_duration) = track_duration {
            duration = duration.max(track_duration);
        }
    }

    log_media_success(path, "probe");
    Ok((tracks, duration))
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
    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|error| map_probe_error(path, error))?;

    let track_id = track.index;
    let track_codec_params = probed
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
        })?
        .codec_params
        .clone();

    let codec_registry = symphonia::default::get_codecs();
    let mut decoder = codec_registry
        .make(&track_codec_params, &DecoderOptions::default())
        .map_err(|error| map_decoder_create_error(path, track.index, error))?;

    let time_base = track_codec_params.time_base;
    let sample_rate = track_codec_params
        .sample_rate
        .filter(|rate| *rate > 0)
        .or_else(|| Some(track.sample_rate).filter(|rate| *rate > 0));

    seek_to_window_start(
        path,
        probed.format.as_mut(),
        track_id,
        window.start,
    )?;
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

    let mut sample_buffer: Option<SampleBuffer<f32>> = None;
    let mut last_reported = 0_u64;
    let mut finished = false;

    loop {
        if finished {
            break;
        }
        if let Some(target) = target_samples {
            if mono_samples.len() >= target {
                break;
            }
        }

        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
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

        if packet.track_id() != track_id {
            continue;
        }

        if let Some(time_base) = time_base {
            if let Some(rate) = resolved_rate {
                let (start_sample, end_sample) = window_sample_bounds(window, rate);
                let packet_start_sample = timestamp_to_sample(packet.ts(), time_base, rate);
                let packet_end_sample =
                    timestamp_to_sample(packet.ts() + packet.dur(), time_base, rate);
                if packet_end_sample <= start_sample {
                    continue;
                }
                if packet_start_sample >= end_sample {
                    break;
                }
            } else {
                let packet_start = timestamp_to_duration(packet.ts(), time_base);
                let packet_end = timestamp_to_duration(packet.ts() + packet.dur(), time_base);
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
            resolved_rate = Some(decoded.spec().rate);
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
            .map(|base| timestamp_to_sample(packet.ts(), base, rate))
            .unwrap_or(start_sample);

        finished = append_frames_in_window(
            decoded,
            &mut WindowCollectContext {
                packet_start_sample,
                window_start_sample: start_sample,
                window_end_sample: end_sample,
                trim_start_frames: packet.trim_start(),
                sample_buffer: &mut sample_buffer,
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
        if shortfall > sample_count_tolerance(rate) {
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
    sample_buffer: &'a mut Option<SampleBuffer<f32>>,
    mono_samples: &'a mut Vec<i16>,
    target_samples: usize,
}

fn append_frames_in_window(
    decoded: AudioBufferRef<'_>,
    ctx: &mut WindowCollectContext<'_>,
) -> bool {
    let spec = *decoded.spec();
    let frame_count = decoded.frames();
    if frame_count == 0 {
        return false;
    }

    let channel_count = spec.channels.count().max(1);
    let required_samples = frame_count * channel_count;

    let needs_new_buffer = ctx
        .sample_buffer
        .as_ref()
        .map(|buffer| buffer.capacity() < required_samples)
        .unwrap_or(true);

    if needs_new_buffer {
        *ctx.sample_buffer = Some(SampleBuffer::<f32>::new(frame_count as u64, spec));
    }

    let buffer = ctx.sample_buffer.as_mut().expect("sample buffer");
    buffer.clear();
    buffer.copy_interleaved_ref(decoded);

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

        let frame = &buffer.samples()[(frame_idx * channel_count)..((frame_idx + 1) * channel_count)];
        let mono = if frame.is_empty() {
            0.0
        } else {
            frame.iter().sum::<f32>() / frame.len() as f32
        };
        ctx.mono_samples.push(float_to_i16(mono));
    }

    false
}

fn timestamp_to_duration(ts: u64, time_base: TimeBase) -> Duration {
    time_to_duration(time_base.calc_time(ts))
}

fn timestamp_to_sample(ts: u64, time_base: TimeBase, sample_rate: u32) -> u64 {
    let time = time_base.calc_time(ts);
    time_to_sample(time_to_duration(time), sample_rate)
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
    let seek_to = SeekTo::Time {
        time: Time::from(start),
        track_id: Some(track_id),
    };

    format
        .seek(SeekMode::Accurate, seek_to)
        .map_err(|error| map_seek_error(path, track_id, start.as_secs_f64(), error))?;

    Ok(())
}

fn sample_count_tolerance(sample_rate: u32) -> usize {
    (sample_rate as usize / 50).max(64)
}

fn track_duration_from_params(params: &symphonia::core::codecs::CodecParameters) -> Option<Duration> {
    let n_frames = params.n_frames?;
    let time_base = params.time_base?;
    Some(time_to_duration(time_base.calc_time(n_frames)))
}

fn time_to_duration(time: Time) -> Duration {
    Duration::new(time.seconds, (time.frac * 1_000_000_000.0) as u32)
}

fn is_audio_track(track: &symphonia::core::formats::Track) -> bool {
    track.codec_params.codec != CODEC_TYPE_NULL
}

fn channel_count(channels: Option<Channels>) -> u16 {
    channels.map(|value| value.count() as u16).unwrap_or(0)
}

fn codec_name(codec: CodecType) -> String {
    match codec {
        CODEC_TYPE_AAC => "aac".into(),
        CODEC_TYPE_MP3 => "mp3".into(),
        CODEC_TYPE_FLAC => "flac".into(),
        CODEC_TYPE_VORBIS => "vorbis".into(),
        CODEC_TYPE_ALAC => "alac".into(),
        CODEC_TYPE_NULL => "unknown".into(),
        _ => format!("{codec:?}"),
    }
}

fn float_to_i16(sample: f32) -> i16 {
    let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i32;
    scaled.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use std::borrow::Cow;

    use hound::{SampleFormat, WavSpec, WavWriter};
    use symphonia::core::audio::{AudioBuffer, AudioBufferRef, Signal, SignalSpec};

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
    fn append_frames_in_window_downmixes_stereo() {
        let spec = SignalSpec::new(44_100, Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
        let mut buffer = AudioBuffer::<f32>::new(2, spec);
        buffer.render_reserved(Some(2));
        buffer.chan_mut(0)[0] = 1.0;
        buffer.chan_mut(1)[0] = -1.0;
        buffer.chan_mut(0)[1] = 0.5;
        buffer.chan_mut(1)[1] = 0.5;

        let mut mono = Vec::new();
        let mut sample_buffer = None;
        append_frames_in_window(
            AudioBufferRef::F32(Cow::Owned(buffer)),
            &mut WindowCollectContext {
                packet_start_sample: 0,
                window_start_sample: 0,
                window_end_sample: 2,
                trim_start_frames: 0,
                sample_buffer: &mut sample_buffer,
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
        let spec = SignalSpec::new(44_100, Channels::FRONT_LEFT);
        let mut buffer = AudioBuffer::<f32>::new(2, spec);
        buffer.render_reserved(Some(2));
        buffer.chan_mut(0)[0] = 1.0;
        buffer.chan_mut(0)[1] = 0.25;

        let mut mono = Vec::new();
        let mut sample_buffer = None;
        append_frames_in_window(
            AudioBufferRef::F32(Cow::Owned(buffer)),
            &mut WindowCollectContext {
                packet_start_sample: 9,
                window_start_sample: 10,
                window_end_sample: 20,
                trim_start_frames: 0,
                sample_buffer: &mut sample_buffer,
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

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn write_container_fixture(
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

    #[test]
    fn probe_and_extract_mkv_container() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tone.mkv");
        if !write_container_fixture(&path, &["-f", "matroska"], &["-c:a", "flac"]) {
            eprintln!("skipping MKV test: ffmpeg unavailable or encode failed");
            return;
        }

        let (tracks, duration) = probe_media(&path).unwrap();
        assert_eq!(tracks.len(), 1);
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

    #[test]
    fn probe_and_extract_mp4_container() {
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
