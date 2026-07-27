use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::application::error::{AlignmentError, FingerprintError, MediaError};
use crate::application::ports::{
    Aligner, Fingerprinter, MediaReader, MediaSession, PcmCorrelator, ProgressReporter,
};
use crate::domain::{
    AudioTrack, ClipMatchEstimate, ClipWindow, Fingerprint, MediaSource, MonoPcmClip,
};

pub struct FakeProgressReporter;

impl ProgressReporter for FakeProgressReporter {
    fn phase(&self, _message: &str) {}

    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

/// Deterministic [`PcmCorrelator`] fake: returns a fixed `(lag_samples, magnitude)` or `None`.
///
/// Use in unit tests that need `AlignVideos` wired with a correlator but don't exercise the
/// PCM refinement path (e.g., `refine_offset_with_pcm: false`). For tests that actually
/// validate PCM lag correction, inject `&FftCorrelator` instead.
pub struct FakePcmCorrelator {
    result: Option<(f64, f64)>,
}

impl FakePcmCorrelator {
    /// Always returns `None` — correlator is wired but never consulted.
    pub fn new() -> Self {
        Self { result: None }
    }
}

impl Default for FakePcmCorrelator {
    fn default() -> Self {
        Self::new()
    }
}

impl PcmCorrelator for FakePcmCorrelator {
    fn cross_correlate_lag(&self, _a: &[f64], _b: &[f64]) -> Option<(f64, f64)> {
        self.result
    }

    fn segment_similarity(&self, _a: &[f64], _b: &[f64]) -> f64 {
        0.0
    }
}

pub struct FakeMediaReader {
    sessions: HashMap<PathBuf, FakeMediaSession>,
    open_calls: Arc<AtomicUsize>,
    open_error: Option<MediaError>,
}

impl Default for FakeMediaReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeMediaReader {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            open_calls: Arc::new(AtomicUsize::new(0)),
            open_error: None,
        }
    }

    pub fn with_session(mut self, path: impl AsRef<Path>, session: FakeMediaSession) -> Self {
        self.sessions.insert(path.as_ref().to_path_buf(), session);
        self
    }

    pub fn with_open_error(mut self, error: MediaError) -> Self {
        self.open_error = Some(error);
        self
    }

    pub fn open_calls(&self) -> usize {
        self.open_calls.load(Ordering::SeqCst)
    }
}

impl MediaReader for FakeMediaReader {
    type Session = FakeMediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError> {
        self.open_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.open_error {
            return Err(error.clone());
        }

        self.sessions
            .get(source.path())
            .cloned()
            .ok_or_else(|| MediaError::FileNotFound(source.path().to_path_buf()))
    }
}

#[derive(Clone)]
pub struct FakeMediaSession {
    tracks: Vec<AudioTrack>,
    extract_error: Option<MediaError>,
    /// When set, extracted PCM is all zeros (fails fingerprint prep / sufficient-audio gate).
    extract_silence: bool,
    /// When set, always returns this clip regardless of the requested window.
    fixed_extract: Option<MonoPcmClip>,
}

impl FakeMediaSession {
    pub fn with_duration(duration: Duration) -> Self {
        Self {
            tracks: vec![test_track(duration)],
            extract_error: None,
            extract_silence: false,
            fixed_extract: None,
        }
    }

    pub fn with_tracks(tracks: Vec<AudioTrack>) -> Self {
        Self {
            tracks,
            extract_error: None,
            extract_silence: false,
            fixed_extract: None,
        }
    }

    pub fn with_extract_error(mut self, error: MediaError) -> Self {
        self.extract_error = Some(error);
        self
    }

    pub fn with_silent_extract(mut self) -> Self {
        self.extract_silence = true;
        self
    }

    pub fn with_fixed_extract(mut self, clip: MonoPcmClip) -> Self {
        self.fixed_extract = Some(clip);
        self
    }
}

impl MediaSession for FakeMediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
        Ok(self.tracks.clone())
    }

    fn extract_mono(
        &mut self,
        _track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MonoPcmClip, MediaError> {
        if let Some(error) = &self.extract_error {
            return Err(error.clone());
        }

        if let Some(clip) = &self.fixed_extract {
            progress.progress(label, 1, 1);
            return Ok(clip.clone());
        }

        progress.progress(label, 1, 1);
        let sample_rate = 44_100;
        let count = window.sample_count_at(sample_rate);
        let samples: Vec<i16> = if self.extract_silence {
            vec![0_i16; count]
        } else {
            (0..count)
                .map(|index| {
                    let t = index as f32 / sample_rate as f32;
                    (f32::sin(440.0 * t * std::f32::consts::TAU) * (i16::MAX as f32 * 0.25)).round()
                        as i16
                })
                .collect()
        };
        Ok(MonoPcmClip {
            sample_rate,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        })
    }
}

pub struct FakeFingerprinter {
    error: Option<FingerprintError>,
    seen_sample_rates: RefCell<Vec<u32>>,
}

impl Default for FakeFingerprinter {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeFingerprinter {
    pub fn new() -> Self {
        Self {
            error: None,
            seen_sample_rates: RefCell::new(Vec::new()),
        }
    }

    pub fn with_error(error: FingerprintError) -> Self {
        Self {
            error: Some(error),
            seen_sample_rates: RefCell::new(Vec::new()),
        }
    }

    pub fn seen_sample_rates(&self) -> Vec<u32> {
        self.seen_sample_rates.borrow().clone()
    }
}

impl Fingerprinter for FakeFingerprinter {
    fn fingerprint(&self, clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }

        self.seen_sample_rates.borrow_mut().push(clip.sample_rate);
        Ok(Fingerprint::new(vec![clip.samples.len() as u32]))
    }
}

pub struct FakeAligner {
    estimates: RefCell<Vec<ClipMatchEstimate>>,
    fallback: ClipMatchEstimate,
    error: Option<AlignmentError>,
}

impl FakeAligner {
    pub fn with_estimate(estimate: ClipMatchEstimate) -> Self {
        Self {
            estimates: RefCell::new(Vec::new()),
            fallback: estimate,
            error: None,
        }
    }

    pub fn with_estimates(estimates: Vec<ClipMatchEstimate>) -> Self {
        let fallback = estimates.last().copied().unwrap_or(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.0,
        });
        Self {
            estimates: RefCell::new(estimates),
            fallback,
            error: None,
        }
    }

    pub fn with_error(error: AlignmentError) -> Self {
        Self {
            estimates: RefCell::new(Vec::new()),
            fallback: ClipMatchEstimate {
                offset_secs: 0.0,
                confidence: 0.0,
            },
            error: Some(error),
        }
    }
}

impl Aligner for FakeAligner {
    fn find_offset(
        &self,
        _left: &Fingerprint,
        _right: &Fingerprint,
    ) -> Result<ClipMatchEstimate, AlignmentError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }

        let mut queue = self.estimates.borrow_mut();
        if queue.len() > 1 {
            return Ok(queue.remove(0));
        }

        Ok(queue.first().copied().unwrap_or(self.fallback))
    }
}

fn test_track(duration: Duration) -> AudioTrack {
    AudioTrack {
        index: 0,
        codec: "test".into(),
        channels: 1,
        sample_rate: 44_100,
        duration: Some(duration),
        decodable: true,
        bit_depth: None,
    }
}
