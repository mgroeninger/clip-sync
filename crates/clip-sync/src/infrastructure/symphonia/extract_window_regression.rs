//! Cross-format regression for `extract_loop` windowed extract (mono + interleaved).
//!
//! Uses a split-tone source (1 s silence, then loud) so mis-seeks are detectable by content,
//! not just sample count. WAV rows run in default CI; encoded containers require `ffmpeg-tests`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};

use super::extract::sample_count_tolerance;
use super::probe::probe_media_reusable;
use super::session::SymphoniaMediaReader;
use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{
    AudioTrack, ClipLabel, ClipWindow, MediaSource, MonoPcmClip, MultiChannelPcm,
};

struct NoopProgress;

impl ProgressReporter for NoopProgress {
    fn phase(&self, _message: &str) {}
    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

const SAMPLE_RATE: u32 = 44_100;
const FILE_SECS: u32 = 4;

#[derive(Debug, Clone, Copy)]
enum WindowKind {
    /// First 500 ms — silence in split-tone fixture.
    StartSilence,
    /// Seconds 1–2 — loud region.
    InteriorLoud,
    /// Last second — loud region, labelled as end clip.
    EndLoud,
}

impl WindowKind {
    fn window(self) -> ClipWindow {
        match self {
            Self::StartSilence => ClipWindow::new(
                Duration::ZERO,
                Duration::from_millis(500),
                ClipLabel::Start,
            ),
            Self::InteriorLoud => ClipWindow::new(
                Duration::from_secs(1),
                Duration::from_secs(2),
                ClipLabel::Interior,
            ),
            Self::EndLoud => ClipWindow::new(
                Duration::from_secs(u64::from(FILE_SECS - 1)),
                Duration::from_secs(u64::from(FILE_SECS)),
                ClipLabel::End,
            ),
        }
    }

    fn expect_loud(self) -> bool {
        !matches!(self, Self::StartSilence)
    }
}

#[derive(Debug, Clone, Copy)]
enum MonoFixtureFormat {
    Wav,
    #[cfg(feature = "ffmpeg-tests")]
    Mp4Aac,
    #[cfg(feature = "ffmpeg-tests")]
    MkvFlac,
    #[cfg(feature = "ffmpeg-tests")]
    MkvAac,
    #[cfg(feature = "ffmpeg-tests")]
    Mp3,
}

impl MonoFixtureFormat {
    fn name(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            #[cfg(feature = "ffmpeg-tests")]
            Self::Mp4Aac => "mp4_aac",
            #[cfg(feature = "ffmpeg-tests")]
            Self::MkvFlac => "mkv_flac",
            #[cfg(feature = "ffmpeg-tests")]
            Self::MkvAac => "mkv_aac",
            #[cfg(feature = "ffmpeg-tests")]
            Self::Mp3 => "mp3",
        }
    }
}

fn write_mono_split_tone_wav(path: &Path, sample_rate: u32, seconds: u32) {
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

fn build_mono_fixture(dir: &Path, format: MonoFixtureFormat) -> Option<PathBuf> {
    let wav = dir.join(format!("{}_source.wav", format.name()));
    write_mono_split_tone_wav(&wav, SAMPLE_RATE, FILE_SECS);

    match format {
        MonoFixtureFormat::Wav => Some(wav),
        #[cfg(feature = "ffmpeg-tests")]
        encoded => {
            use crate::test_support::ffmpeg_util::{self, EncodeFormat};

            let encode_format = match encoded {
                MonoFixtureFormat::Mp4Aac => EncodeFormat::Mp4Aac,
                MonoFixtureFormat::MkvFlac => EncodeFormat::MkvFlac,
                MonoFixtureFormat::MkvAac => EncodeFormat::MkvAac,
                MonoFixtureFormat::Mp3 => EncodeFormat::Mp3,
                MonoFixtureFormat::Wav => unreachable!(),
            };
            let ext = match encoded {
                MonoFixtureFormat::Mp4Aac => "mp4",
                MonoFixtureFormat::MkvFlac | MonoFixtureFormat::MkvAac => "mkv",
                MonoFixtureFormat::Mp3 => "mp3",
                MonoFixtureFormat::Wav => unreachable!(),
            };
            let out = dir.join(format!("split_tone.{ext}"));
            if !ffmpeg_util::encode_audio(&wav, &out, encode_format) {
                return None;
            }
            Some(out)
        }
    }
}

#[cfg(feature = "ffmpeg-tests")]
fn build_stereo_mp4_fixture(dir: &Path) -> Option<PathBuf> {
    use crate::test_support::ffmpeg_util::{self, EncodeFormat};

    let wav = dir.join("stereo_source.wav");
    write_stereo_split_tone_wav(&wav, SAMPLE_RATE, FILE_SECS);
    let out = dir.join("split_tone_stereo.mp4");
    if !ffmpeg_util::encode_audio(&wav, &out, EncodeFormat::Mp4AacStereo) {
        return None;
    }
    Some(out)
}

fn session_extract_mono(path: &Path, track: &AudioTrack, window: &ClipWindow, label: &str) -> MonoPcmClip {
    let reader = SymphoniaMediaReader;
    let mut session = reader
        .open(&MediaSource::new(path))
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    session
        .extract_mono(track, window, &NoopProgress, label)
        .unwrap_or_else(|error| panic!("mono extract {label} from {}: {error}", path.display()))
}

fn session_extract_interleaved(
    path: &Path,
    track: &AudioTrack,
    window: &ClipWindow,
    label: &str,
) -> MultiChannelPcm {
    let reader = SymphoniaMediaReader;
    let mut session = reader
        .open(&MediaSource::new(path))
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
    session
        .extract_interleaved(track, window, &NoopProgress, label)
        .unwrap_or_else(|error| panic!("interleaved extract {label} from {}: {error}", path.display()))
}

fn peak_abs(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample.abs()).fold(0.0f32, f32::max)
}

fn peak_abs_i16(samples: &[i16]) -> f32 {
    samples.iter().map(|&s| s.abs() as f32 / 32767.0).fold(0.0f32, f32::max)
}

fn assert_sample_count(actual: usize, expected: usize, rate: u32, context: &str) {
    assert!(
        (actual as i64 - expected as i64).abs() <= sample_count_tolerance(rate) as i64,
        "{context}: got {actual} samples, expected ~{expected} (rate={rate})"
    );
}

fn assert_content_oracle(peak: f32, expect_loud: bool, lossy: bool, context: &str) {
    let loud_threshold = if lossy { 200.0_f32 / 32767.0 } else { 1_000.0 / 32767.0 };
    let silence_threshold = if lossy { 2_000.0_f32 / 32767.0 } else { 100.0 / 32767.0 };
    if expect_loud {
        assert!(
            peak > loud_threshold,
            "{context}: expected loud window, peak={peak} (threshold={loud_threshold})"
        );
    } else {
        assert!(
            peak < silence_threshold,
            "{context}: expected silent window, peak={peak} (threshold={silence_threshold})"
        );
    }
}

fn run_mono_window_matrix(format: MonoFixtureFormat) {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = build_mono_fixture(temp.path(), format).unwrap_or_else(|| {
        panic!(
            "failed to build {} mono split-tone fixture (is ffmpeg available?)",
            format.name()
        )
    });
    let lossy = !matches!(format, MonoFixtureFormat::Wav);

    let (tracks, _, _) = probe_media_reusable(&path).expect("probe");
    assert_eq!(tracks.len(), 1, "{}: expected one audio track", format.name());
    assert!(
        tracks[0].decodable,
        "{}: track should be decodable",
        format.name()
    );
    let track = &tracks[0];
    let rate = track.sample_rate;

    for kind in [WindowKind::StartSilence, WindowKind::InteriorLoud, WindowKind::EndLoud] {
        let window = kind.window();
        let label = format!("{}_{:?}", format.name(), kind);
        let clip = session_extract_mono(&path, track, &window, &label);
        assert_eq!(clip.sample_rate, rate, "{label}: sample rate");
        let expected = window.sample_count_at(rate);
        assert_sample_count(clip.samples.len(), expected, rate, &label);
        assert_content_oracle(
            peak_abs_i16(&clip.samples),
            kind.expect_loud(),
            lossy,
            &label,
        );
    }
}

fn run_interleaved_window_matrix(path: &Path, lossy: bool, label_prefix: &str) {
    let (tracks, _, _) = probe_media_reusable(path).expect("probe");
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].channels, 2, "{label_prefix}: expected stereo");
    let track = &tracks[0];
    let rate = track.sample_rate;

    for kind in [WindowKind::StartSilence, WindowKind::InteriorLoud, WindowKind::EndLoud] {
        let window = kind.window();
        let label = format!("{label_prefix}_{kind:?}");
        let pcm = session_extract_interleaved(path, track, &window, &label);
        assert_eq!(pcm.sample_rate, rate);
        assert_eq!(pcm.channels, 2);
        let expected_frames = window.sample_count_at(rate);
        assert_sample_count(pcm.frames(), expected_frames, rate, &label);
        assert_content_oracle(peak_abs(&pcm.samples), kind.expect_loud(), lossy, &label);
    }
}

#[test]
fn extract_window_regression_wav_mono() {
    run_mono_window_matrix(MonoFixtureFormat::Wav);
}

#[test]
fn extract_window_regression_wav_interleaved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("stereo_split.wav");
    write_stereo_split_tone_wav(&path, SAMPLE_RATE, FILE_SECS);
    run_interleaved_window_matrix(&path, false, "wav_stereo");
}

#[cfg(feature = "ffmpeg-tests")]
#[test]
fn extract_window_regression_encoded_mono() {
    use crate::test_support::ffmpeg_util;

    if !ffmpeg_util::ffmpeg_available() {
        eprintln!("skipping encoded extract window regression: ffmpeg unavailable");
        return;
    }

    for format in [
        MonoFixtureFormat::Mp4Aac,
        MonoFixtureFormat::MkvFlac,
        MonoFixtureFormat::MkvAac,
        MonoFixtureFormat::Mp3,
    ] {
        run_mono_window_matrix(format);
    }
}

#[cfg(feature = "ffmpeg-tests")]
#[test]
fn extract_window_regression_mp4_stereo_interleaved() {
    use crate::test_support::ffmpeg_util;

    if !ffmpeg_util::ffmpeg_available() {
        eprintln!("skipping MP4 stereo interleaved regression: ffmpeg unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let path = build_stereo_mp4_fixture(temp.path()).expect("encode stereo MP4 AAC");
    run_interleaved_window_matrix(&path, true, "mp4_aac_stereo");
}

/// Long MKV/AAC file with a backward seek to the shared-timeline end window — the scenario
/// that failed on production MKV/AAC before seek-boundary tail padding.
///
/// Uses a 120 s end clip on a 240/360 s pair: MKV/AAC seek imprecision is ~3–4 s absolute,
/// which only clears the 95 % padding threshold once the clip is long enough (~2 min).
#[cfg(feature = "ffmpeg-tests")]
#[test]
fn mkv_aac_anchored_end_window_extract_succeeds() {
    use crate::application::testing::audio_fixtures::write_anchored_end_symmetric_pair;
    use crate::test_support::ffmpeg_util::{self, EncodeFormat};

    if !ffmpeg_util::ffmpeg_available() {
        eprintln!("skipping MKV/AAC anchored end extract: ffmpeg unavailable");
        return;
    }

    const SHARED_SECS: u32 = 240;
    const LONG_SECS: u32 = 360;
    const CLIP_SECS: u64 = 120;
    const OFFSET_SECS: u32 = 3;

    let temp = tempfile::tempdir().expect("tempdir");
    let (_wav_a, wav_b) = write_anchored_end_symmetric_pair(
        temp.path(),
        11_025,
        SHARED_SECS,
        LONG_SECS,
        OFFSET_SECS,
    );
    let path_b = temp.path().join("b.mkv");
    assert!(
        ffmpeg_util::encode_audio(&wav_b, &path_b, EncodeFormat::MkvAac),
        "MKV/AAC encode failed"
    );

    let (tracks, _, _) = probe_media_reusable(&path_b).expect("probe b.mkv");
    assert!(tracks[0].decodable, "MKV/AAC track should be decodable");

    let end = Duration::from_secs(u64::from(SHARED_SECS));
    let start = end - Duration::from_secs(CLIP_SECS);
    let end_window = ClipWindow::new(start, end, ClipLabel::End);

    let reader = SymphoniaMediaReader;
    let mut session = reader
        .open(&MediaSource::new(&path_b))
        .expect("open session");
    let tracks = session.list_tracks().expect("list tracks");

    let start_window = ClipWindow::new(Duration::ZERO, Duration::from_secs(10), ClipLabel::Start);
    session
        .extract_mono(&tracks[0], &start_window, &NoopProgress, "warmup")
        .expect("start warmup extract before backward seek to end");

    let clip = session
        .extract_mono(&tracks[0], &end_window, &NoopProgress, "anchored end")
        .expect("MKV/AAC end clip extract should succeed after long seek");

    let rate = tracks[0].sample_rate;
    let expected = end_window.sample_count_at(rate);
    assert_sample_count(
        clip.samples.len(),
        expected,
        rate,
        "mkv_aac anchored end window",
    );
    let peak = peak_abs_i16(&clip.samples);
    assert!(
        peak > 100.0 / 32767.0,
        "end window should contain chirp audio, peak={peak}"
    );
}

/// M-FDK-RESET regression: after a backward seek on a reused session, the FDK
/// decoder must re-prime to a **bit-identical steady state** — no SBR/overlap carry
/// from a previously-decoded window may survive into the converged output.
///
/// Oracle & why the steady-state region: extracting window B on a session that just
/// decoded a later window A is compared against a fresh-session extract of B. The
/// leading frames legitimately differ between the two (the backward seek re-primes
/// SBR from a different reader position — the `reset_decode_io` domain, not
/// `reset()`), so we assert on the region *after* the re-prime transient. There the
/// two must match exactly. With the pre-fix no-op `reset()`, A's decoder state bleeds
/// through and thousands of steady-state samples diverge (low magnitude but
/// systematic); the recreate-decoder `reset()` drives that to zero. A fresh-vs-fresh
/// control guards against unrelated nondeterminism. The in-process HE-AAC sweep
/// encoder means this runs on stock ffmpeg (no libfdk needed). Requires `he-aac` +
/// `ffmpeg-tests`.
#[cfg(all(feature = "he-aac", feature = "ffmpeg-tests"))]
#[test]
fn fdk_reset_backward_seek_reprimes_to_identical_steady_state() {
    use crate::test_support::ffmpeg_util;

    // Skip the leading re-prime transient (a few 2048-sample HE-AAC frames); assert
    // bit-exact convergence beyond it.
    const STEADY_STATE_FROM: usize = 8_192;

    let temp = tempfile::tempdir().expect("tempdir");
    let mp4 = temp.path().join("sweep_he_aac.mp4");
    // Genuine HE-AAC (SBR) 1 kHz -> 16 kHz sweep so window A (late) and window B
    // (early) carry very different high-band content.
    if !ffmpeg_util::write_he_aac_sweep_mp4(&mp4, SAMPLE_RATE, 3, 1_000.0, 16_000.0) {
        eprintln!("skipping FDK reset regression: ffmpeg unavailable for HE-AAC remux");
        return;
    }

    let (tracks, _, _) = probe_media_reusable(&mp4).expect("probe sweep mp4");
    let track = tracks.into_iter().next().expect("one audio track");

    // A: late window. B: earlier window, reached by a backward seek after A.
    let window_a = ClipWindow::new(
        Duration::from_millis(2_400),
        Duration::from_millis(2_900),
        ClipLabel::End,
    );
    let window_b = ClipWindow::new(
        Duration::from_millis(600),
        Duration::from_millis(1_100),
        ClipLabel::Interior,
    );

    // One extract per fresh session, replaying `windows` in order on a single session.
    let extract = |windows: &[&ClipWindow]| -> Vec<i16> {
        let reader = SymphoniaMediaReader;
        let mut session = reader.open(&MediaSource::new(&mp4)).expect("open session");
        let mut clip = None;
        for window in windows {
            clip = Some(
                session
                    .extract_mono(&track, window, &NoopProgress, "reset_regression")
                    .expect("mono extract"),
            );
        }
        clip.expect("at least one window").samples
    };

    let fresh_b = extract(&[&window_b]);
    let fresh_b_again = extract(&[&window_b]);
    // Everyday cached-decoder path: decode later window A, then backward-seek to B —
    // this runs `decoder.reset()` between windows.
    let reused_b = extract(&[&window_a, &window_b]);

    let steady_divergent = |a: &[i16], b: &[i16]| -> usize {
        let n = a.len().min(b.len());
        (STEADY_STATE_FROM.min(n)..n).filter(|&i| a[i] != b[i]).count()
    };

    // Control: decoding is deterministic, so two fresh extracts of B are identical.
    assert_eq!(
        steady_divergent(&fresh_b, &fresh_b_again),
        0,
        "fresh-vs-fresh control diverged: extract is nondeterministic, oracle invalid"
    );
    assert!(
        fresh_b.len() > STEADY_STATE_FROM,
        "window B too short ({} samples) to have a steady-state region",
        fresh_b.len()
    );

    // The fix: the reused decoder must re-prime to the same steady state as fresh.
    let divergent = steady_divergent(&fresh_b, &reused_b);
    assert_eq!(
        divergent, 0,
        "after a backward seek, {divergent} steady-state samples (beyond \
         {STEADY_STATE_FROM}) differ from a fresh extract; FDK reset() is leaking \
         decoder state across the seek"
    );
}
