//! Phase 0 — oxideav AC-3 decode quality vs ffmpeg on spectrally rich corpus chirp.
//!
//! Fixture: same 300–700 Hz sweep as `tests/corpus` chirp WAVs, ffmpeg-encoded to AC-3
//! (256 kb/s @ 48 kHz stereo — production-like). Sine-only fixtures do not trigger the
//! oxideav railing bug; this test does when the library is affected.
//!
//! Run: `cargo test -p clip-sync --features ac3,ffmpeg-tests ac3_corpus_chirp -- --nocapture`
//!
//! Known baseline (oxideav-ac3 0.0.8 crates.io): ~3000 full-scale samples per 60 s stereo clip;
//! fixed in oxideav-ac3 0.0.9+ (crates.io 0.0.10 includes the fix).

use std::path::Path;
use std::time::Duration;

use crate::application::ports::{MediaReader, MediaSession, ProgressReporter};
use crate::domain::{ClipLabel, ClipWindow, MediaSource};
use crate::infrastructure::symphonia::probe::probe_media_reusable;
use crate::infrastructure::symphonia::session::SymphoniaMediaReader;
use crate::test_support::ac3_pcm_analysis::{
    peak_abs, peak_abs_f32, railed_sample_count, railed_sample_count_f32,
};
use crate::test_support::audio_fixtures::write_corpus_chirp_wav;
use crate::test_support::ffmpeg_util::{
    encode_wav_to_ac3_mp4, extract_pcm_s16le_ffmpeg, ffmpeg_available,
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BIT_RATE_KBPS: u32 = 256;
/// Match corpus default clip length — long enough to hit intermittent railing.
const DURATION_SECS: u32 = 60;

struct NoopProgress;

impl ProgressReporter for NoopProgress {
    fn phase(&self, _message: &str) {}
    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

fn session_extract_interleaved(
    path: &Path,
    track: &crate::domain::AudioTrack,
    window: &ClipWindow,
) -> crate::domain::MultiChannelPcm {
    let reader = SymphoniaMediaReader;
    let mut session = reader
        .open(&MediaSource::new(path))
        .unwrap_or_else(|error| panic!("open session for {}: {error}", path.display()));
    session
        .extract_interleaved(track, window, &NoopProgress, "ac3 chirp characterization")
        .unwrap_or_else(|error| panic!("oxideav extract from {}: {error}", path.display()))
}

fn build_corpus_chirp_ac3_mp4(dir: &Path) -> Option<std::path::PathBuf> {
    if !ffmpeg_available() {
        return None;
    }

    let wav = dir.join("corpus_chirp.wav");
    let mp4 = dir.join("corpus_chirp_ac3.mp4");
    write_corpus_chirp_wav(&wav, SAMPLE_RATE, DURATION_SECS);
    if !encode_wav_to_ac3_mp4(&wav, &mp4, SAMPLE_RATE, CHANNELS, BIT_RATE_KBPS) {
        return None;
    }
    Some(mp4)
}

#[test]
fn oxideav_ac3_corpus_chirp_ffmpeg_reference_has_no_railed_samples() {
    let temp = tempfile::tempdir().unwrap();
    let Some(path) = build_corpus_chirp_ac3_mp4(temp.path()) else {
        eprintln!("skipping: ffmpeg unavailable or AC-3 encode failed");
        return;
    };

    let reference =
        extract_pcm_s16le_ffmpeg(&path, SAMPLE_RATE, CHANNELS, 0.0, f64::from(DURATION_SECS))
            .expect("ffmpeg reference decode should succeed");

    let railed = railed_sample_count(&reference);
    let peak = peak_abs(&reference);
    assert_eq!(
        railed, 0,
        "ffmpeg AC-3 reference should have zero full-scale samples (peak={peak}); \
         fixture or encode path is bad if this fails"
    );
    assert!(
        peak > 1_000,
        "ffmpeg reference should be non-silent (peak={peak})"
    );
}

#[test]
fn oxideav_ac3_corpus_chirp_decode_has_no_railed_samples() {
    let temp = tempfile::tempdir().unwrap();
    let Some(path) = build_corpus_chirp_ac3_mp4(temp.path()) else {
        eprintln!("skipping: ffmpeg unavailable or AC-3 encode failed");
        return;
    };

    let (tracks, _, _) = probe_media_reusable(&path).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].codec, "ac3");
    assert!(
        tracks[0].decodable,
        "AC-3 track should be decodable with ac3 feature enabled"
    );

    let window = ClipWindow::new(
        Duration::ZERO,
        Duration::from_secs(u64::from(DURATION_SECS)),
        ClipLabel::Start,
    );
    let pcm = session_extract_interleaved(&path, &tracks[0], &window);

    assert_eq!(pcm.sample_rate, SAMPLE_RATE);
    assert_eq!(pcm.channels, CHANNELS);

    let ffmpeg_pcm =
        extract_pcm_s16le_ffmpeg(&path, SAMPLE_RATE, CHANNELS, 0.0, f64::from(DURATION_SECS))
            .expect("ffmpeg reference decode");

    let oxideav_railed = railed_sample_count_f32(&pcm.samples);
    let ffmpeg_railed = railed_sample_count(&ffmpeg_pcm);
    let oxideav_peak = peak_abs_f32(&pcm.samples);
    let ffmpeg_peak = peak_abs(&ffmpeg_pcm);

    eprintln!(
        "AC-3 corpus chirp @ {BIT_RATE_KBPS} kb/s {SAMPLE_RATE} Hz {CHANNELS}ch x {DURATION_SECS}s: \
         oxideav railed={oxideav_railed}/{} peak={oxideav_peak} | \
         ffmpeg railed={ffmpeg_railed}/{} peak={ffmpeg_peak}",
        pcm.samples.len(),
        ffmpeg_pcm.len(),
    );

    assert_eq!(
        ffmpeg_railed, 0,
        "sanity: ffmpeg reference must be clean (got {ffmpeg_railed} railed)"
    );
    assert_eq!(
        oxideav_railed,
        0,
        "oxideav-ac3 decode of corpus chirp AC-3 should have zero full-scale (|s|>=1.0) \
         samples; got {oxideav_railed} railed of {} total (peak={oxideav_peak}).",
        pcm.samples.len()
    );
}
