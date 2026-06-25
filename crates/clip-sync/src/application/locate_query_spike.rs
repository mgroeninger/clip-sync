//! Phase Q0 spike — query-reference localization (see
//! `docs/archive/query-reference-alignment-plan.md` § Phase Q0).
//!
//! Goal: prove the **ring-buffer sliding window** coarse search finds a known anchor
//! within the coarse tolerance (±2 s) while holding live PCM to `O(L)` (one window),
//! not `O(file)`. This module is test-only scaffolding — the production use case
//! (`LocateQueryInReference`) lands in Q1. It also records the empirical facts the plan
//! asks for (windows scored, ring high-water) and pins down the **offset sign**, which the
//! plan's first-pass pseudocode got backwards.
//!
//! ## Synthetic media
//!
//! Reference A is a single monotonic-frequency chirp defined as a pure function of the
//! absolute sample index, so contiguous scan buckets concatenate seamlessly and the query B
//! is an *exact* slice of A at a known anchor. The monotonic sweep makes every window's
//! spectral content distinct, so the true anchor is unambiguous.

#![cfg(test)]

use std::collections::VecDeque;
use std::f64::consts::TAU;
use std::time::Duration;

use crate::application::error::MediaError;
use crate::application::ports::{Aligner, Fingerprinter, MediaSession, ProgressReporter};
use crate::application::testing::fakes::FakeProgressReporter;
use crate::domain::policies::ClipPlanningOptions;
use crate::domain::{
    clip_windows_with_options, AudioTrack, ClipPlan, ClipWindow, MediaExtent, MonoPcmClip,
};
use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};

/// Fingerprint sample rate for the spike (Chromaprint-native; keeps the long file cheap).
const RATE: u32 = 11_025;

// Linear chirp: instantaneous frequency f0 + k·t sweeps F0 → F1 across `SWEEP_SECS`, staying
// below Nyquist for the whole reference so there is no aliasing and no spectral repetition.
const F0_HZ: f64 = 200.0;
const F1_HZ: f64 = 5_000.0;
const SWEEP_SECS: f64 = 900.0;

fn chirp_sample(index: u64) -> i16 {
    let t = index as f64 / f64::from(RATE);
    let k = (F1_HZ - F0_HZ) / SWEEP_SECS;
    // Phase of a linear chirp: 2π·(f0·t + ½·k·t²). Instantaneous freq = f0 + k·t.
    let phase = TAU * (F0_HZ * t + 0.5 * k * t * t);
    (phase.sin() * (f64::from(i16::MAX) * 0.5)).round() as i16
}

fn chirp_clip(start_index: u64, sample_count: usize) -> MonoPcmClip {
    let samples = (0..sample_count)
        .map(|offset| chirp_sample(start_index + offset as u64))
        .collect();
    MonoPcmClip {
        sample_rate: RATE,
        samples,
        decode_error_skips: 0,
        decoded_sample_count: None,
    }
}

fn secs_to_samples(secs: f64) -> usize {
    (secs * f64::from(RATE)).round() as usize
}

/// Fake reference session that synthesizes chirp PCM on demand — A is never held in full,
/// so the ring high-water measured below reflects only the sliding window, not the file.
struct ChirpReferenceSession {
    duration_secs: f64,
}

impl ChirpReferenceSession {
    fn track(&self) -> AudioTrack {
        AudioTrack {
            index: 0,
            codec: "pcm".into(),
            channels: 1,
            sample_rate: RATE,
            duration: Some(Duration::from_secs_f64(self.duration_secs)),
            decodable: true,
            bit_depth: None,
        }
    }
}

impl MediaSession for ChirpReferenceSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
        Ok(vec![self.track()])
    }

    fn extract_mono(
        &mut self,
        _track: &AudioTrack,
        window: &ClipWindow,
        _progress: &dyn ProgressReporter,
        _label: &str,
    ) -> Result<MonoPcmClip, MediaError> {
        let start_index = (window.start.as_secs_f64() * f64::from(RATE)).round() as u64;
        let count = window.sample_count_at(RATE);
        Ok(chirp_clip(start_index, count))
    }
}

/// Result of the spike coarse search.
struct CoarseLocateOutcome {
    /// Best anchor: A timeline position where query t=0 aligns.
    anchor_a_secs: f64,
    confidence: f32,
    windows_scored: u32,
    /// Peak live PCM samples held in the ring buffer during the whole scan.
    ring_high_water_samples: usize,
}

/// Ring-buffer sliding-window coarse search, mirroring the plan's
/// § "Coarse search (reference timeline)" — but with the corrected offset sign.
///
/// `bucket_secs` is the decode granularity; `l_secs` the window length; `stride_secs` the
/// advance between scores. Memory stays `O(l_secs)`: the ring never holds more than one
/// window plus the in-flight bucket.
fn coarse_locate(
    session: &mut ChirpReferenceSession,
    query_fp: &crate::domain::Fingerprint,
    l_secs: f64,
    stride_secs: f64,
    bucket_secs: f64,
) -> CoarseLocateOutcome {
    let track = session.track();
    let aligner = ChromaprintAligner::default();
    let fingerprinter = ChromaprintFingerprinter::default();

    let l_samples = secs_to_samples(l_secs);
    let stride_samples = secs_to_samples(stride_secs) as u64;

    let mut ring: VecDeque<i16> = VecDeque::new();
    let mut ring_start_index: u64 = 0; // absolute sample index of ring.front()
    let mut next_score_index: u64 = 0; // absolute sample index of next window's start
    let mut best: Option<(f64, f32)> = None;
    let mut windows_scored: u32 = 0;
    let mut ring_high_water: usize = 0;

    // Drop ring samples before `keep_from` (we never look back past the next window start).
    let trim_front = |ring: &mut VecDeque<i16>, ring_start_index: &mut u64, keep_from: u64| {
        while *ring_start_index < keep_from && !ring.is_empty() {
            ring.pop_front();
            *ring_start_index += 1;
        }
    };

    session
        .scan_mono_buckets(
            &track,
            bucket_secs,
            &FakeProgressReporter,
            "spike",
            &mut |bucket| {
                ring.extend(bucket.pcm.samples.iter().copied());
                trim_front(&mut ring, &mut ring_start_index, next_score_index);
                ring_high_water = ring_high_water.max(ring.len());

                loop {
                    let window_end_index = next_score_index + l_samples as u64;
                    let ring_end_index = ring_start_index + ring.len() as u64;
                    if window_end_index > ring_end_index {
                        break; // not enough buffered yet for a full window
                    }

                    let offset_in_ring = (next_score_index - ring_start_index) as usize;
                    let samples: Vec<i16> = ring
                        .iter()
                        .skip(offset_in_ring)
                        .take(l_samples)
                        .copied()
                        .collect();
                    let window_clip = MonoPcmClip {
                        sample_rate: RATE,
                        samples,
                        decode_error_skips: 0,
                        decoded_sample_count: None,
                    };
                    let window_fp = fingerprinter.fingerprint(&window_clip).unwrap();
                    let estimate = aligner.find_offset(&window_fp, query_fp).unwrap();

                    let pos_secs = next_score_index as f64 / f64::from(RATE);
                    // find_offset(left=window, right=query) returns r with
                    //   query_local = window_local + r.
                    // window_local = a_abs - pos, so query_local = a_abs + (r - pos).
                    // The anchor (a_abs where query_local = 0) is therefore pos - r.
                    // NB: plan v1 pseudocode said `pos + r` — wrong sign; this spike confirms.
                    let anchor = pos_secs - estimate.offset_secs;
                    windows_scored += 1;
                    if best.is_none_or(|(_, c)| estimate.confidence > c) {
                        best = Some((anchor, estimate.confidence));
                    }

                    next_score_index += stride_samples;
                    trim_front(&mut ring, &mut ring_start_index, next_score_index);
                }

                Ok::<(), MediaError>(())
            },
        )
        .expect("chirp scan should not fail");

    let (anchor_a_secs, confidence) = best.expect("at least one window must be scored");
    CoarseLocateOutcome {
        anchor_a_secs,
        confidence,
        windows_scored,
        ring_high_water_samples: ring_high_water,
    }
}

// Spike fixture parameters.
const A_SECS: f64 = 900.0; // 15 min reference
const B_SECS: f64 = 180.0; // 3 min query
const ANCHOR_SECS: f64 = 540.0; // 9:00 — where B was sliced from A
const STRIDE_SECS: f64 = 60.0;
const BUCKET_SECS: f64 = 10.0;
const COARSE_TOLERANCE_SECS: f64 = 2.0; // coarse tier (no PCM refine in the spike)
const MIN_LOCALIZE_CONFIDENCE: f32 = 0.3;

fn query_fingerprint() -> crate::domain::Fingerprint {
    let query_clip = chirp_clip(secs_to_samples(ANCHOR_SECS) as u64, secs_to_samples(B_SECS));
    ChromaprintFingerprinter::default()
        .fingerprint(&query_clip)
        .unwrap()
}

// Fingerprinting a 15-min reference is ~15 s; this is a one-time Q0 de-risking spike, not
// permanent coverage (Q1 adds right-sized `locate_query_*` tests). Run on demand:
//   cargo test -p clip-sync --lib locate_query_spike -- --ignored --nocapture
#[test]
#[ignore = "Q0 spike — expensive; run explicitly with --ignored"]
fn coarse_search_finds_known_anchor() {
    let mut session = ChirpReferenceSession { duration_secs: A_SECS };
    let query_fp = query_fingerprint();

    let outcome = coarse_locate(&mut session, &query_fp, B_SECS, STRIDE_SECS, BUCKET_SECS);

    assert!(
        (outcome.anchor_a_secs - ANCHOR_SECS).abs() <= COARSE_TOLERANCE_SECS,
        "anchor {} not within ±{COARSE_TOLERANCE_SECS}s of {ANCHOR_SECS}",
        outcome.anchor_a_secs
    );
    assert!(
        outcome.confidence >= MIN_LOCALIZE_CONFIDENCE,
        "confidence {} below localize threshold {MIN_LOCALIZE_CONFIDENCE}",
        outcome.confidence
    );

    // Record the empirical facts the plan asks for (visible with `--nocapture`).
    let expected_windows = ((A_SECS - B_SECS) / STRIDE_SECS).ceil() as u32 + 1;
    eprintln!(
        "Q0 spike: anchor={:.3}s confidence={:.3} windows_scored={} (≈expected {}) \
         ring_high_water={} samples ({:.1}s)",
        outcome.anchor_a_secs,
        outcome.confidence,
        outcome.windows_scored,
        expected_windows,
        outcome.ring_high_water_samples,
        outcome.ring_high_water_samples as f64 / f64::from(RATE),
    );
}

#[test]
#[ignore = "Q0 spike — expensive; run explicitly with --ignored"]
fn coarse_search_ring_buffer_bounded_memory() {
    let mut session = ChirpReferenceSession { duration_secs: A_SECS };
    let query_fp = query_fingerprint();

    let outcome = coarse_locate(&mut session, &query_fp, B_SECS, STRIDE_SECS, BUCKET_SECS);

    // O(L), not O(file): the ring holds at most one window plus the in-flight bucket.
    let window_samples = secs_to_samples(B_SECS);
    let bucket_samples = secs_to_samples(BUCKET_SECS);
    let bound = window_samples + 2 * bucket_samples;
    assert!(
        outcome.ring_high_water_samples <= bound,
        "ring high-water {} exceeded O(L) bound {} (window {} + 2·bucket {})",
        outcome.ring_high_water_samples,
        bound,
        window_samples,
        bucket_samples
    );
    // And demonstrably far below holding the whole reference.
    let full_file_samples = secs_to_samples(A_SECS);
    assert!(
        outcome.ring_high_water_samples * 2 < full_file_samples,
        "ring high-water {} is not meaningfully smaller than the full file {}",
        outcome.ring_high_water_samples,
        full_file_samples
    );
}

#[test]
fn symmetric_clip_plan_counts_mismatch_on_long_short_pair() {
    // The failure this feature exists to fix: symmetric multi-clip planning yields different
    // window counts for a long reference vs a short query, which `align_extracted_pair`
    // rejects as a clip-count mismatch.
    let plan = ClipPlan::new(Duration::from_secs(200), 2);
    let opts = ClipPlanningOptions::default();
    let extent_a = MediaExtent::from_declared(Duration::from_secs_f64(A_SECS));
    let extent_b = MediaExtent::from_declared(Duration::from_secs_f64(B_SECS));

    let windows_a = clip_windows_with_options(&extent_a, &plan, opts).unwrap();
    let windows_b = clip_windows_with_options(&extent_b, &plan, opts).unwrap();

    assert_ne!(
        windows_a.len(),
        windows_b.len(),
        "expected clip-count mismatch (A={}, B={})",
        windows_a.len(),
        windows_b.len()
    );
}
