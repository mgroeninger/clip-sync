use std::f32::consts::TAU;
use std::path::Path;
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use symphonia::core::audio::{layouts, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, FinalizeResult};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::packet::PacketRef;

use super::extract::{
    append_frames_in_window, append_interleaved_frames_in_window, decode_shortfall_limit,
    extract_interleaved_with_state, extract_mono_with_state, float_to_i16, sample_count_tolerance,
    window_sample_bounds, InterleavedCollectContext, WindowCollectContext,
};
use super::probe::probe_media_reusable;
use super::session::{CachedTrackDecoder, MediaIoState, SymphoniaMediaReader};
use crate::infrastructure::symphonia::session::ensure_track_decoder;
use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{
    AudioTrack, ClipLabel, ClipWindow, InterleavedScanBucket, MediaSource, MonoPcmClip,
    MonoScanBucket, MultiChannelPcm,
};

struct NoopProgress;

impl ProgressReporter for NoopProgress {
    fn phase(&self, _message: &str) {}
    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

fn session_extract_mono(
    path: &Path,
    track: &AudioTrack,
    window: &ClipWindow,
    label: &str,
) -> MonoPcmClip {
    let reader = SymphoniaMediaReader;
    let session = reader
        .open(&MediaSource::new(path))
        .unwrap_or_else(|error| panic!("open session for {}: {error}", path.display()));
    session
        .extract_mono(track, window, &NoopProgress, label)
        .unwrap_or_else(|error| panic!("extract from {}: {error}", path.display()))
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
fn decode_shortfall_limit_allows_tail_gap_on_long_clips() {
    let target = 43_200_000_usize;
    let limit = decode_shortfall_limit(48_000, target, true);
    assert!(limit >= 76_304, "limit was {limit}");
    assert!(limit <= 96_000, "limit was {limit}");
}

#[test]
fn decode_shortfall_limit_stays_strict_without_tail_padding() {
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
    let mut scratch = Vec::new();
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
        &mut scratch,
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
    let mut scratch = Vec::new();
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
        &mut scratch,
    );

    assert_eq!(mono.len(), 1);
    assert!(mono[0] > 0);
}

#[test]
fn probe_and_extract_wav() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone.wav");
    write_test_wav(&path, 44_100, 3);

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
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
    let clip = session_extract_mono(&path, &tracks[0], &window, "test");

    assert_eq!(clip.sample_rate, 44_100);
    assert_eq!(clip.samples.len(), 44_100);
}

#[test]
fn session_open_reuses_probe_format_reader() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("open_reuse.wav");
    write_test_wav(&path, 44_100, 2);

    let reader = SymphoniaMediaReader;
    let session = reader.open(&MediaSource::new(&path)).unwrap();
    assert!(
        session.has_io_state(),
        "open should retain format reader from probe (no second probe on first extract)"
    );

    let tracks = session.list_tracks().unwrap();
    let window = ClipWindow::new(
        Duration::from_secs(0),
        Duration::from_secs(1),
        ClipLabel::Start,
    );
    session
        .extract_mono(&tracks[0], &window, &NoopProgress, "clip")
        .unwrap();
    assert_eq!(session.cached_decoder_count(), 1);
}

#[test]
fn session_reuses_format_reader_across_extracts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("reuse.wav");
    write_test_wav(&path, 44_100, 4);

    let reader = SymphoniaMediaReader;
    let session = reader.open(&MediaSource::new(&path)).unwrap();
    let tracks = session.list_tracks().unwrap();

    assert!(session.has_io_state());

    let start_window = ClipWindow::new(
        Duration::from_secs(0),
        Duration::from_secs(1),
        ClipLabel::Start,
    );
    let end_window = ClipWindow::new(
        Duration::from_secs(2),
        Duration::from_secs(3),
        ClipLabel::End,
    );

    let first = session
        .extract_mono(&tracks[0], &start_window, &NoopProgress, "start")
        .unwrap();
    assert!(session.has_io_state());
    assert_eq!(session.cached_decoder_count(), 1);

    let second = session
        .extract_mono(&tracks[0], &end_window, &NoopProgress, "end")
        .unwrap();
    assert_eq!(session.cached_decoder_count(), 1);
    assert_eq!(first.samples.len(), 44_100);
    assert_eq!(second.samples.len(), 44_100);
}

#[test]
fn scan_mono_buckets_emits_full_sample_windows() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bucket_scan.wav");
    write_test_wav(&path, 44_100, 4);

    let reader = SymphoniaMediaReader;
    let session = reader.open(&MediaSource::new(&path)).unwrap();
    let tracks = session.list_tracks().unwrap();

    let mut buckets = Vec::new();
    let mut collect = |bucket: MonoScanBucket| -> Result<(), MediaError> {
        buckets.push(bucket);
        Ok(())
    };

    session
        .scan_mono_buckets(&tracks[0], 1.0, &NoopProgress, "scan", &mut collect)
        .unwrap();

    assert_eq!(buckets.len(), 4);
    for bucket in &buckets[..3] {
        assert_eq!(bucket.pcm.samples.len(), 44_100);
        assert!((bucket.end_secs - bucket.start_secs - 1.0).abs() < 0.001);
    }
    assert_eq!(buckets[3].pcm.samples.len(), 44_100);
}

#[test]
fn scan_interleaved_buckets_emits_full_frame_windows() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("interleaved_bucket_scan.wav");
    write_test_wav(&path, 44_100, 4);

    let reader = SymphoniaMediaReader;
    let session = reader.open(&MediaSource::new(&path)).unwrap();
    let tracks = session.list_tracks().unwrap();

    let mut buckets = Vec::new();
    let mut collect = |bucket: InterleavedScanBucket| -> Result<(), MediaError> {
        buckets.push(bucket);
        Ok(())
    };

    session
        .scan_interleaved_buckets(&tracks[0], 1.0, &NoopProgress, "scan", &mut collect)
        .unwrap();

    assert_eq!(buckets.len(), 4);
    for bucket in &buckets[..3] {
        assert_eq!(bucket.pcm.channels, 2);
        assert_eq!(bucket.pcm.frames(), 44_100);
        assert_eq!(bucket.pcm.samples.len(), 44_100 * 2);
        assert!((bucket.end_secs - bucket.start_secs - 1.0).abs() < 0.001);
    }
    assert_eq!(buckets[3].pcm.frames(), 44_100);
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

    let (tracks, _, _) = probe_media_reusable(&path).unwrap();
    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let clip = session_extract_mono(&path, &tracks[0], &window, "test");

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
#[test]
fn probe_and_extract_mkv_container() {
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone.mkv");
    if !ffmpeg_util::write_lavfi_sine_container(&path, &["-f", "matroska"], &["-c:a", "flac"], 3) {
        eprintln!("skipping MKV test: ffmpeg unavailable or encode failed");
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].decodable);
    assert!(tracks[0].duration.unwrap().as_secs() >= 2);
    assert!(duration.as_secs() >= 2);

    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let clip = session_extract_mono(&path, &tracks[0], &window, "mkv");
    let expected = tracks[0].sample_rate as usize;
    assert!((clip.samples.len() as i64 - expected as i64).abs() <= sample_count_tolerance(tracks[0].sample_rate) as i64);
}

#[cfg(feature = "ffmpeg-tests")]
#[test]
fn probe_and_extract_mp4_container() {
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone.mp4");
    if !ffmpeg_util::write_lavfi_sine_container(
        &path,
        &["-f", "mp4"],
        &["-c:a", "aac", "-b:a", "128k"],
        3,
    ) {
        eprintln!("skipping MP4 test: ffmpeg unavailable or encode failed");
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].decodable);
    assert!(tracks[0].duration.unwrap().as_secs() >= 2);
    assert!(duration.as_secs() >= 2);

    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let clip = session_extract_mono(&path, &tracks[0], &window, "mp4");
    let expected = tracks[0].sample_rate as usize;
    assert!((clip.samples.len() as i64 - expected as i64).abs() <= sample_count_tolerance(tracks[0].sample_rate) as i64);
}

#[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
#[test]
fn probe_and_extract_he_aac_mp4_container() {
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone-he-aac.mp4");
    if !ffmpeg_util::write_he_aac_mp4_fixture(&path) {
        eprintln!("skipping HE-AAC MP4 test: ffmpeg unavailable or HE-AAC encode failed");
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
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
    let clip = session_extract_mono(&path, &tracks[0], &window, "he-aac mp4");
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
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone-he-aac-51.mp4");
    if !ffmpeg_util::write_he_aac_surround_mp4_fixture(&path) {
        eprintln!(
            "skipping HE-AAC surround MP4 test: ffmpeg unavailable or HE-AAC 5.1 encode failed"
        );
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
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
    let clip = session_extract_mono(&path, &tracks[0], &window, "he-aac surround mp4");
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

fn session_extract_interleaved(
    path: &Path,
    track: &AudioTrack,
    window: &ClipWindow,
    label: &str,
) -> MultiChannelPcm {
    let reader = SymphoniaMediaReader;
    let session = reader
        .open(&MediaSource::new(path))
        .unwrap_or_else(|error| panic!("open session for {}: {error}", path.display()));
    session
        .extract_interleaved(track, window, &NoopProgress, label)
        .unwrap_or_else(|error| panic!("interleaved extract from {}: {error}", path.display()))
}

/// Stereo WAV with a distinct left (440 Hz tone) and right (silent) channel, so a downmix bug
/// would be detectable: a correct interleaved extract keeps the right channel at zero.
fn write_stereo_distinct_wav(path: &Path, sample_rate: u32, seconds: u32) {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let total_frames = sample_rate as u64 * seconds as u64;

    for index in 0..total_frames {
        let t = index as f32 / sample_rate as f32;
        let left = ((TAU * 440.0 * t).sin() * (i16::MAX as f32 * 0.8)) as i16;
        writer.write_sample(left).unwrap();
        writer.write_sample(0_i16).unwrap();
    }

    writer.finalize().unwrap();
}

/// Stereo WAV: first second silent on both channels, second second a loud tone on both.
fn write_stereo_split_tone_wav(path: &Path, sample_rate: u32, seconds: u32) {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let total_frames = sample_rate as u64 * seconds as u64;

    for index in 0..total_frames {
        let sample = if index < sample_rate as u64 {
            0_i16
        } else {
            i16::MAX / 2
        };
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }

    writer.finalize().unwrap();
}

#[test]
fn append_interleaved_frames_in_window_keeps_channels() {
    use symphonia::core::audio::{layouts, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef};

    let spec = AudioSpec::new(44_100, layouts::CHANNEL_LAYOUT_STEREO);
    let mut buffer = AudioBuffer::<f32>::new(spec, 2);
    buffer.render_silence(Some(2));
    buffer.plane_mut(0).unwrap()[0] = 1.0;
    buffer.plane_mut(1).unwrap()[0] = -1.0;
    buffer.plane_mut(0).unwrap()[1] = 0.5;
    buffer.plane_mut(1).unwrap()[1] = 0.25;

    let mut out = Vec::new();
    let mut scratch = Vec::new();
    append_interleaved_frames_in_window(
        GenericAudioBufferRef::F32(&buffer),
        &mut InterleavedCollectContext {
            packet_start_frame: 0,
            window_start_frame: 0,
            window_end_frame: 2,
            trim_start_frames: 0,
            out: &mut out,
            channels: 2,
            target_frames: 2,
        },
        &mut scratch,
    );

    // Two frames × two channels, interleaved, with no downmix.
    assert_eq!(out.len(), 4);
    assert!(out[0] > 0, "frame 0 left should be positive");
    assert!(out[1] < 0, "frame 0 right should be negative");
    assert!(out[2] > 0 && out[3] > 0, "frame 1 both channels positive");
    assert!(out[2] > out[3], "left (0.5) should exceed right (0.25)");
}

#[test]
fn extract_interleaved_preserves_stereo_channels() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("stereo_distinct.wav");
    write_stereo_distinct_wav(&path, 44_100, 3);

    let (tracks, _, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks[0].channels, 2);

    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let clip = session_extract_interleaved(&path, &tracks[0], &window, "stereo");

    assert_eq!(clip.sample_rate, 44_100);
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.frames(), 44_100);
    assert_eq!(clip.samples.len(), 44_100 * 2);

    let left_peak = clip.samples.iter().step_by(2).map(|s| s.abs()).max().unwrap_or(0);
    let right_peak = clip.samples.iter().skip(1).step_by(2).map(|s| s.abs()).max().unwrap_or(0);
    assert!(left_peak > 1_000, "left channel should carry the tone, peak={left_peak}");
    assert_eq!(right_peak, 0, "right channel was silent; downmix would leak energy here");
}

#[test]
fn extract_interleaved_midfile_window_seeks_correctly() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("stereo_split.wav");
    write_stereo_split_tone_wav(&path, 44_100, 2);

    let (tracks, _, _) = probe_media_reusable(&path).unwrap();
    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let clip = session_extract_interleaved(&path, &tracks[0], &window, "split");

    assert_eq!(clip.channels, 2);
    assert!(
        (clip.frames() as i64 - 44_100).abs() <= sample_count_tolerance(44_100) as i64
    );
    let peak = clip.samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    assert!(peak > 1_000, "mid-file window should contain the loud second half, peak={peak}");
}

#[test]
fn extract_interleaved_default_port_method_is_unsupported() {
    struct BareSession;
    impl MediaSession for BareSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(Vec::new())
        }
        fn extract_mono(
            &self,
            _track: &AudioTrack,
            _window: &ClipWindow,
            _progress: &dyn ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            unreachable!("not exercised")
        }
    }

    let session = BareSession;
    let track = AudioTrack {
        index: 0,
        codec: "pcm".into(),
        channels: 2,
        sample_rate: 44_100,
        bitrate: None,
        duration: Some(Duration::from_secs(1)),
        decodable: true,
    };
    let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(1), ClipLabel::Start);
    match session.extract_interleaved(&track, &window, &NoopProgress, "x") {
        Err(MediaError::Unsupported(_)) => {}
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

/// Wraps a real decoder and injects `DecodeError` on the specified 1-based call numbers.
/// Used to test that `decode_error_skips` is counted correctly without needing a corrupt file.
struct InjectingDecoder {
    inner: Box<dyn AudioDecoder>,
    fail_on_calls: Vec<usize>,
    call_count: usize,
}

impl InjectingDecoder {
    fn new(inner: Box<dyn AudioDecoder>, fail_on_calls: Vec<usize>) -> Self {
        Self { inner, fail_on_calls, call_count: 0 }
    }
}

static INJECTING_CODEC_INFO: CodecInfo = CodecInfo {
    short_name: "inject",
    long_name: "injecting decoder for tests",
    profiles: &[],
};

impl AudioDecoder for InjectingDecoder {
    fn reset(&mut self) {
        self.inner.reset();
    }

    fn codec_info(&self) -> &CodecInfo {
        &INJECTING_CODEC_INFO
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        self.inner.codec_params()
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> symphonia::core::errors::Result<GenericAudioBufferRef<'_>> {
        self.call_count += 1;
        if self.fail_on_calls.contains(&self.call_count) {
            return Err(SymphoniaError::DecodeError("injected fault"));
        }
        self.inner.decode_ref(packet)
    }

    fn finalize(&mut self) -> FinalizeResult {
        self.inner.finalize()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.inner.last_decoded()
    }
}

#[test]
fn mono_and_interleaved_same_skip_count_on_corrupt_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("both_skip.wav");
    write_test_wav(&path, 44_100, 2);

    let make_state_with_injector = |fail_on: Vec<usize>| {
        let (tracks, _, io) = probe_media_reusable(&path).unwrap();
        let mut state = io.unwrap_or_else(|| MediaIoState::open(&path).unwrap());
        let track = tracks.into_iter().next().unwrap();
        ensure_track_decoder(&path, &mut state, &track).unwrap();
        let real = state.decoders.remove(&track.index).unwrap();
        state.decoders.insert(
            track.index,
            CachedTrackDecoder {
                decoder: Box::new(InjectingDecoder::new(real.decoder, fail_on)),
                time_base: real.time_base,
            },
        );
        (track, state)
    };

    let (mono_track, mut mono_state) = make_state_with_injector(vec![3, 5]);
    let (interleaved_track, mut interleaved_state) = make_state_with_injector(vec![3, 5]);

    let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(1), ClipLabel::Start);
    let mono_result =
        extract_mono_with_state(&path, &mut mono_state, &mono_track, &window, &NoopProgress, "m")
            .unwrap();
    let interleaved_result = extract_interleaved_with_state(
        &path,
        &mut interleaved_state,
        &interleaved_track,
        &window,
        &NoopProgress,
        "i",
    )
    .unwrap();

    assert_eq!(
        mono_result.decode_error_skips,
        interleaved_result.decode_error_skips,
        "mono ({}) and interleaved ({}) reported different skip counts on the same corrupt fixture",
        mono_result.decode_error_skips,
        interleaved_result.decode_error_skips,
    );
    assert_eq!(mono_result.decode_error_skips, 2);
}

#[test]
fn extract_mono_decode_errors_are_counted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decode_skip_mono.wav");
    write_test_wav(&path, 44_100, 2);

    let (tracks, _, io) = probe_media_reusable(&path).unwrap();
    let mut state = io.unwrap_or_else(|| MediaIoState::open(&path).unwrap());
    let track = &tracks[0];

    ensure_track_decoder(&path, &mut state, track).unwrap();

    let real = state.decoders.remove(&track.index).unwrap();
    state.decoders.insert(
        track.index,
        CachedTrackDecoder {
            decoder: Box::new(InjectingDecoder::new(real.decoder, vec![3, 5])),
            time_base: real.time_base,
        },
    );

    let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(1), ClipLabel::Start);
    let result =
        extract_mono_with_state(&path, &mut state, track, &window, &NoopProgress, "test")
            .unwrap();

    assert_eq!(
        result.decode_error_skips, 2,
        "expected exactly 2 injected skips, got {}",
        result.decode_error_skips
    );
    assert!(!result.samples.is_empty(), "should still produce audio after decode errors");
}

#[test]
fn extract_interleaved_decode_errors_are_counted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decode_skip.wav");
    write_test_wav(&path, 44_100, 2);

    let (tracks, _, io) = probe_media_reusable(&path).unwrap();
    let mut state = io.unwrap_or_else(|| MediaIoState::open(&path).unwrap());
    let track = &tracks[0];

    ensure_track_decoder(&path, &mut state, track).unwrap();

    let real = state.decoders.remove(&track.index).unwrap();
    state.decoders.insert(
        track.index,
        CachedTrackDecoder {
            decoder: Box::new(InjectingDecoder::new(real.decoder, vec![3, 5])),
            time_base: real.time_base,
        },
    );

    let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(1), ClipLabel::Start);
    let result =
        extract_interleaved_with_state(&path, &mut state, track, &window, &NoopProgress, "test")
            .unwrap();

    assert_eq!(
        result.decode_error_skips, 2,
        "expected exactly 2 injected skips, got {}",
        result.decode_error_skips
    );
    assert!(result.frames() > 0, "should still produce audio after decode errors");
    assert_eq!(result.channels, 2, "channel count should be preserved");
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

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
#[test]
fn probe_and_extract_ac3_surround_mp4() {
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone-ac3-51.mp4");
    if !ffmpeg_util::write_ac3_surround_mp4_fixture(&path) {
        eprintln!("skipping AC-3 5.1 MP4 test: ffmpeg unavailable or AC-3 encode failed");
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].codec, "ac3");
    assert_eq!(
        tracks[0].channels, 6,
        "expected 5.1 channel layout, got {} channels",
        tracks[0].channels
    );
    assert!(
        tracks[0].decodable,
        "AC-3 5.1 track should be decodable when the ac3 feature is enabled"
    );
    assert!(tracks[0].duration.unwrap().as_secs() >= 2);
    assert!(duration.as_secs() >= 2);

    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let pcm = session_extract_interleaved(&path, &tracks[0], &window, "ac3 5.1 mp4");
    let expected = tracks[0].sample_rate as usize;
    assert!(
        (pcm.frames() as i64 - expected as i64).abs()
            <= sample_count_tolerance(tracks[0].sample_rate) as i64
    );
    assert_eq!(pcm.channels, 6, "interleaved extract should preserve all 6 channels");
    let peak = pcm.samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    assert!(peak > 100, "expected non-silent PCM from AC-3 5.1 decode, peak={peak}");
}

#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]
#[test]
fn probe_and_extract_eac3_surround_mp4() {
    use crate::application::testing::ffmpeg_util;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone-eac3-51.mp4");
    if !ffmpeg_util::write_eac3_surround_mp4_fixture(&path) {
        eprintln!("skipping E-AC-3 5.1 MP4 test: ffmpeg unavailable or E-AC-3 encode failed");
        return;
    }

    let (tracks, duration, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].codec, "eac3");
    assert_eq!(
        tracks[0].channels, 6,
        "expected 5.1 channel layout, got {} channels",
        tracks[0].channels
    );
    assert!(
        tracks[0].decodable,
        "E-AC-3 5.1 track should be decodable when the ac3 feature is enabled"
    );
    assert!(tracks[0].duration.unwrap().as_secs() >= 2);
    assert!(duration.as_secs() >= 2);

    let window = ClipWindow::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        ClipLabel::Interior,
    );
    let pcm = session_extract_interleaved(&path, &tracks[0], &window, "eac3 5.1 mp4");
    let expected = tracks[0].sample_rate as usize;
    assert!(
        (pcm.frames() as i64 - expected as i64).abs()
            <= sample_count_tolerance(tracks[0].sample_rate) as i64
    );
    assert_eq!(pcm.channels, 6, "interleaved extract should preserve all 6 channels");
    let peak = pcm.samples.iter().map(|s| s.abs()).max().unwrap_or(0);
    assert!(peak > 100, "expected non-silent PCM from E-AC-3 5.1 decode, peak={peak}");
}
