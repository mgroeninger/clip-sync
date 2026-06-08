use std::f32::consts::TAU;
use std::path::Path;
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use symphonia::core::audio::{layouts, AudioBuffer, AudioMut, AudioSpec, GenericAudioBufferRef};

use super::extract::{
    append_frames_in_window, decode_shortfall_limit, float_to_i16, sample_count_tolerance,
    window_sample_bounds, WindowCollectContext,
};
use super::probe::probe_media_reusable;
use super::session::SymphoniaMediaReader;
use crate::application::error::MediaError;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{AudioTrack, ClipLabel, ClipWindow, MediaSource, MonoPcmClip};

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
