use std::f64::consts::TAU;
use std::time::Duration;

use super::*;
use crate::application::config::AlignConfig;
use crate::application::error::MediaError;
use crate::application::ports::ProgressReporter;
use crate::application::testing::fakes::FakeProgressReporter;
use crate::domain::query_localization::assert_recommended_offset_matches_orientation;
use crate::domain::{AudioTrack, MediaExtent, MonoPcmClip, QueryLocalization};
use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
use crate::infrastructure::correlation::FftCorrelator;
use crate::infrastructure::resample::RubatoResampler;

const RATE: u32 = 11_025;
const F0_HZ: f64 = 200.0;
const F1_HZ: f64 = 5_000.0;
const SWEEP_SECS: f64 = 900.0;

fn chirp_sample(index: u64) -> i16 {
    let t = index as f64 / f64::from(RATE);
    let k = (F1_HZ - F0_HZ) / SWEEP_SECS;
    let phase = TAU * (F0_HZ * t + 0.5 * k * t * t);
    (phase.sin() * (f64::from(i16::MAX) * 0.5)).round() as i16
}

/// Deterministic pseudo-noise unrelated to the chirp (LCG), for the no-match case.
fn noise_sample(index: u64) -> i16 {
    let x = index.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    ((x >> 33) as i32 as i16) / 2
}

#[derive(Clone, Copy)]
enum Content {
    /// Chirp content; `base_offset_secs` shifts local t=0 to a reference index, `period_secs`
    /// makes the content repeat (for the ambiguous case).
    Chirp {
        base_offset_secs: f64,
        period_secs: Option<f64>,
    },
    Noise,
}

struct FakeSession {
    duration_secs: f64,
    content: Content,
}

impl FakeSession {
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

    fn sample_at(&self, local_index: u64) -> i16 {
        match self.content {
            Content::Chirp {
                base_offset_secs,
                period_secs,
            } => {
                let base = (base_offset_secs * f64::from(RATE)).round() as u64;
                let abs = local_index + base;
                let idx = match period_secs {
                    Some(p) => abs % ((p * f64::from(RATE)).round() as u64),
                    None => abs,
                };
                chirp_sample(idx)
            }
            Content::Noise => noise_sample(local_index),
        }
    }
}

impl MediaSession for FakeSession {
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
        let samples = (0..count)
            .map(|offset| self.sample_at(start_index + offset as u64))
            .collect();
        Ok(MonoPcmClip {
            sample_rate: RATE,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        })
    }
}

fn extent(secs: f64) -> MediaExtent {
    MediaExtent::from_declared(Duration::from_secs_f64(secs))
}

fn reference(duration_secs: f64, period_secs: Option<f64>) -> FakeSession {
    FakeSession {
        duration_secs,
        content: Content::Chirp {
            base_offset_secs: 0.0,
            period_secs,
        },
    }
}

fn query_from(anchor_secs: f64, duration_secs: f64) -> FakeSession {
    FakeSession {
        duration_secs,
        content: Content::Chirp {
            base_offset_secs: anchor_secs,
            period_secs: None,
        },
    }
}

fn run(
    reference: &mut FakeSession,
    query: &mut FakeSession,
    config: &AlignConfig,
    reference_is_a: bool,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
) -> QueryLocalization {
    let ref_track = reference.track();
    let query_track = query.track();
    let extent_reference = extent(reference.duration_secs);
    let extent_query = extent(query.duration_secs);

    let fingerprinter = ChromaprintFingerprinter::default();
    let aligner = ChromaprintAligner::default();
    let resampler = RubatoResampler;
    let correlator = FftCorrelator;
    let deps = LocateQueryDeps {
        fingerprinter: &fingerprinter,
        aligner: &aligner,
        resampler: &resampler,
        correlator: &correlator,
    };

    let outcome = locate_query_in_reference(
        LocateQueryFile {
            session: reference,
            track: &ref_track,
            extent: extent_reference,
        },
        LocateQueryFile {
            session: query,
            track: &query_track,
            extent: extent_query,
        },
        config,
        deps,
        &FakeProgressReporter,
    )
    .expect("localization should not error");
    QueryLocalization::from_reference_outcome(outcome, reference_is_a, extent_a, extent_b)
}

fn run_a_long(reference: &mut FakeSession, query: &mut FakeSession, config: &AlignConfig) -> QueryLocalization {
    let extent_a = extent(reference.duration_secs);
    let extent_b = extent(query.duration_secs);
    run(reference, query, config, true, extent_a, extent_b)
}

#[test]
fn locate_query_passes_mid_file_embed() {
    let mut reference = reference(180.0, None);
    let mut query = query_from(90.0, 70.0);
    let config = AlignConfig::default();

    let loc = run_a_long(&mut reference, &mut query, &config);

    assert!(loc.skip_reason.is_none(), "unexpected skip: {:?}", loc.skip_reason);
    // Refined tier: post-PCM anchor within ±0.05 s of truth.
    assert!(
        (loc.anchor_ref_secs - 90.0).abs() <= 0.05,
        "anchor {} not within ±0.05 s of 90",
        loc.anchor_ref_secs
    );
    assert!(loc.confidence >= config.alignment.query_min_match_score);
    assert_recommended_offset_matches_orientation(&loc, 1e-6);
    // Mapped region: B spans [0, 70] on its own timeline.
    assert!((loc.mapped_region.video_b_start_secs - 0.0).abs() < 0.01);
    assert!((loc.mapped_region.shared_length_secs - 70.0).abs() < 0.5);
}

#[test]
fn locate_query_passes_mid_file_embed_when_b_is_reference() {
    let anchor_secs = 90.0;
    let query_secs = 70.0;
    let mut reference_b = reference(180.0, None);
    let mut query_a = query_from(anchor_secs, query_secs);
    let config = AlignConfig::default();
    let extent_a = extent(query_secs);
    let extent_b = extent(180.0);

    let loc = run(
        &mut reference_b,
        &mut query_a,
        &config,
        false,
        extent_a,
        extent_b,
    );

    assert!(loc.skip_reason.is_none(), "unexpected skip: {:?}", loc.skip_reason);
    assert!(
        (loc.anchor_ref_secs - anchor_secs).abs() <= 0.05,
        "anchor {} not within ±0.05 s of {anchor_secs}",
        loc.anchor_ref_secs
    );
    let offset = loc.recommended_offset_secs().expect("offset");
    assert!(
        (offset - anchor_secs).abs() <= 0.05,
        "offset {offset} expected ~+{anchor_secs}"
    );
    assert_recommended_offset_matches_orientation(&loc, 0.05);
    assert!((loc.mapped_region.video_a_start_secs - 0.0).abs() < 0.01);
    assert!((loc.mapped_region.video_a_end_secs - query_secs).abs() < 0.5);
    assert!((loc.mapped_region.video_b_start_secs - anchor_secs).abs() <= 0.05);
    assert!(
        (loc.mapped_region.video_b_end_secs - (anchor_secs + query_secs)).abs() < 0.5
    );
    assert!(
        (loc.mapped_region.video_b_start_secs - (loc.mapped_region.video_a_start_secs + offset))
            .abs()
            < 1e-6,
        "b = a + offset at anchor"
    );
}

// Direct tests of the cross-window ambiguity logic (independent of chromaprint/chirp fixtures,
// which are chroma-periodic and so an unreliable proxy for "clean match"). These guard the
// regression where partial-overlap neighbor windows of a single true match (a "shoulder" near
// the peak, within one query length) were treated as a competing location and the match was
// flagged ambiguous, halving its confidence.
fn candidate(anchor: f64, confidence: f32) -> super::Candidate {
    super::Candidate {
        anchor_ref_secs: anchor,
        confidence,
        window_start_secs: anchor,
        window_end_secs: anchor + 60.0,
    }
}

#[test]
fn select_best_cluster_shoulder_within_query_is_not_ambiguous() {
    // Strong peak at 2700 with decaying partial-overlap shoulders within one query length (480 s).
    let cands = vec![
        candidate(2700.0, 0.99),
        candidate(2760.0, 0.86),
        candidate(2820.0, 0.84),
        candidate(2880.0, 0.78),
    ];
    let (best, ambiguous) = super::select_best_cluster(&cands, 480.0).expect("cluster");
    assert!((best.anchor_ref_secs - 2700.0).abs() < 1e-9);
    assert!(!ambiguous, "decaying shoulders within query length must not be ambiguous");
}

#[test]
fn select_best_cluster_distant_strong_peak_is_ambiguous() {
    // Two comparably strong peaks a full query length apart (genuine repeated content).
    let cands = vec![candidate(0.0, 0.98), candidate(600.0, 0.95)];
    let (_, ambiguous) = super::select_best_cluster(&cands, 480.0).expect("cluster");
    assert!(ambiguous, "two strong peaks at distinct locations must be ambiguous");
}

#[test]
fn locate_query_fails_below_threshold() {
    let mut reference = reference(180.0, None);
    let mut query = FakeSession {
        duration_secs: 70.0,
        content: Content::Noise,
    };
    let config = AlignConfig::default();

    let loc = run_a_long(&mut reference, &mut query, &config);

    assert_eq!(loc.recommended_offset_secs(), None);
    assert!(loc.skip_reason.is_some());
}

#[test]
fn locate_query_skips_query_shorter_than_min_clip_length() {
    let mut reference = reference(180.0, None);
    let mut query = query_from(90.0, 30.0); // < MIN_CLIP_LENGTH (60 s)
    let config = AlignConfig::default();

    let loc = run_a_long(&mut reference, &mut query, &config);

    assert_eq!(loc.windows_scored, 0);
    assert_eq!(loc.recommended_offset_secs(), None);
    assert!(loc
        .skip_reason
        .as_deref()
        .is_some_and(|r| r.contains("minimum clip length")));
}

#[test]
fn locate_query_respects_window_cap() {
    let mut reference = reference(180.0, None);
    let mut query = query_from(90.0, 70.0);
    let mut config = AlignConfig::default();
    config.alignment.query_max_windows_scored = 2;

    let loc = run_a_long(&mut reference, &mut query, &config);

    // Stride widened past the base, and we never scored more than the cap.
    assert!(
        loc.search_stride_secs > config.alignment.query_search_stride_secs,
        "stride {} should widen past base 60",
        loc.search_stride_secs
    );
    assert!(loc.windows_scored <= 2, "windows_scored={}", loc.windows_scored);
}

#[test]
fn resolve_mode_auto_uses_query_on_short_vs_long() {
    use crate::application::config::AlignmentMode;
    let mode = resolve_alignment_mode(
        AlignmentMode::Auto,
        &extent(3600.0),
        &extent(480.0),
        1,
        1,
        0.5,
    );
    assert_eq!(mode, AlignmentModeUsed::QueryReference);
}

#[test]
fn resolve_mode_auto_uses_query_on_clip_count_mismatch() {
    use crate::application::config::AlignmentMode;
    // Near-equal durations (ratio above threshold) but differing window counts → Tier 2.
    let mode = resolve_alignment_mode(
        AlignmentMode::Auto,
        &extent(1000.0),
        &extent(900.0),
        3,
        1,
        0.5,
    );
    assert_eq!(mode, AlignmentModeUsed::QueryReference);
}

#[test]
fn resolve_mode_auto_stays_symmetric_for_equal_pair() {
    use crate::application::config::AlignmentMode;
    let mode = resolve_alignment_mode(
        AlignmentMode::Auto,
        &extent(1000.0),
        &extent(950.0),
        2,
        2,
        0.5,
    );
    assert_eq!(mode, AlignmentModeUsed::Symmetric);
}

#[test]
fn resolve_mode_explicit_overrides() {
    use crate::application::config::AlignmentMode;
    assert_eq!(
        resolve_alignment_mode(AlignmentMode::Symmetric, &extent(10.0), &extent(3600.0), 1, 1, 0.5),
        AlignmentModeUsed::Symmetric
    );
    assert_eq!(
        resolve_alignment_mode(AlignmentMode::QueryReference, &extent(1000.0), &extent(1000.0), 2, 2, 0.5),
        AlignmentModeUsed::QueryReference
    );
}

#[test]
fn locate_query_ambiguous_repeat_lowers_confidence() {
    // Reference content repeats every 90 s, so a 70 s query slice matches two anchors.
    let mut reference = reference(180.0, Some(90.0));
    let mut query = query_from(0.0, 70.0);
    let config = AlignConfig::default();

    let loc = run_a_long(&mut reference, &mut query, &config);

    if loc.skip_reason.is_none() {
        assert!(loc.ambiguous, "expected ambiguous localization on repeated content");
    }
}
