//! `LocateQueryInReference` — Phase Q1 core of query-reference alignment.
//!
//! Searches a short *query* clip (the shorter file) against a long *reference* timeline (the
//! longer file) and returns a [`ReferenceLocalizationOutcome`]: anchor and coarse-search facts
//! on the reference timeline. Callers frame A/B roles via
//! [`QueryLocalization::from_reference_outcome`]. See `docs/archive/query-reference-alignment-plan.md` § Q1.
//!
//! The coarse search is the ring-buffer sliding window proven in the Q0 spike: a small fixed
//! `bucket_secs` decode granularity feeds a length-`L` ring buffer that is fingerprinted every
//! `stride`, keeping live PCM at `O(L)` rather than `O(file)`.

use std::collections::VecDeque;
use std::time::Duration;

use tracing::warn;

use crate::application::config::{AlignConfig, AlignmentMode, MIN_CLIP_LENGTH};
use crate::application::error::AlignmentError;
use crate::application::offset_refinement::refine_offset_around_prior;
use crate::application::ports::{
    Aligner, Fingerprinter, MediaSession, PcmCorrelator, ProgressReporter, Resampler,
};
use crate::domain::pcm_preparation::{prepare_clip_for_fingerprint, PcmPreparationOptions};
use crate::domain::{
    should_use_query_mode, AlignmentModeUsed, AudioTrack, ClipLabel, ClipMatchEstimate, ClipWindow,
    MediaExtent, MonoPcmClip, ReferenceLocalizationOutcome,
};

/// Anchors within this many seconds are treated as the same localization cluster (coarse tier).
const ANCHOR_CLUSTER_TOLERANCE_SECS: f64 = 2.0;
/// A second cluster scoring at least this fraction of the best (at a different anchor) is ambiguous.
const AMBIGUITY_SCORE_FRACTION: f32 = 0.75;
/// Confidence multiplier applied to an ambiguous localization.
const AMBIGUOUS_CONFIDENCE_FACTOR: f32 = 0.5;

/// Resolve which algorithm to run from the configured [`AlignmentMode`] and the two files.
///
/// For `Auto`, callers pass the symmetric clip-window counts (`windows_a`/`windows_b`); these
/// are only consulted in Tier 2 (near-equal durations), so callers may compute clip plans
/// lazily after a Tier-1 ratio check if they want to avoid the work.
pub fn resolve_alignment_mode(
    mode: AlignmentMode,
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    windows_a: usize,
    windows_b: usize,
    query_min_duration_ratio: f64,
) -> AlignmentModeUsed {
    let use_query = match mode {
        AlignmentMode::QueryReference => true,
        AlignmentMode::Symmetric => false,
        AlignmentMode::Auto => should_use_query_mode(
            extent_a,
            extent_b,
            windows_a,
            windows_b,
            query_min_duration_ratio,
        ),
    };
    if use_query {
        AlignmentModeUsed::QueryReference
    } else {
        AlignmentModeUsed::Symmetric
    }
}

/// One file participating in query-reference localization.
pub struct LocateQueryFile<'a, S> {
    pub session: &'a mut S,
    pub track: &'a AudioTrack,
    pub extent: MediaExtent,
}

/// Injected ports for localization (mirrors the symmetric path's dependency set).
pub struct LocateQueryDeps<'a> {
    pub fingerprinter: &'a dyn Fingerprinter,
    pub aligner: &'a dyn Aligner,
    pub resampler: &'a dyn Resampler,
    pub correlator: &'a dyn PcmCorrelator,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    anchor_ref_secs: f64,
    confidence: f32,
    window_start_secs: f64,
    window_end_secs: f64,
}

/// Localize `query` (shorter file) on the `reference` (longer file) timeline.
///
/// Returns a [`ReferenceLocalizationOutcome`] in reference-timeline terms; a skipped/no-match
/// outcome carries `skip_reason` rather than erroring (callers may still scan the target file).
pub fn locate_query_in_reference<RS, QS>(
    reference: LocateQueryFile<'_, RS>,
    query: LocateQueryFile<'_, QS>,
    config: &AlignConfig,
    deps: LocateQueryDeps<'_>,
    progress: &dyn ProgressReporter,
) -> Result<ReferenceLocalizationOutcome, AlignmentError>
where
    RS: MediaSession,
    QS: MediaSession,
{
    let stride_base = config.alignment.query_search_stride_secs;
    let min_clip_secs = MIN_CLIP_LENGTH.as_secs_f64();

    // 1. Extract + prepare the full query clip.
    let prep = PcmPreparationOptions {
        normalize_loudness: config.clip.normalize_loudness,
        trim_silence: config.clip.trim_silence,
        window_slide_secs: 0,
    };
    let query_effective = query.extent.effective();
    if query_effective.as_secs_f64() < min_clip_secs {
        return Ok(ReferenceLocalizationOutcome::skipped(
            "query shorter than minimum clip length",
            0,
            stride_base,
        ));
    }
    let query_window = ClipWindow::new(Duration::ZERO, query_effective, ClipLabel::Start);
    let raw_query = query
        .session
        .extract_mono(query.track, &query_window, progress, "query")
        .map_err(|e| AlignmentError::EngineFailed(format!("query extract failed: {e}")))?;
    let prepared_query = match prepare_clip_for_fingerprint(&raw_query, prep) {
        Ok(clip) => clip,
        Err(_) => {
            return Ok(ReferenceLocalizationOutcome::skipped(
                "query had insufficient audio after preparation",
                0,
                stride_base,
            ));
        }
    };

    // L = clamp(query_prepared_duration, MIN_CLIP_LENGTH, clip_length). Keep L >= query by
    // truncating an over-long query to clip_length on both sides (window and fingerprint).
    let clip_length_secs = config.clip.clip_length.as_secs_f64();
    let query_secs = prepared_query.duration_secs();
    if query_secs < min_clip_secs {
        return Ok(ReferenceLocalizationOutcome::skipped(
            "query shorter than minimum clip length after preparation",
            0,
            stride_base,
        ));
    }
    let l_secs = query_secs.min(clip_length_secs);
    let query_clip = truncate_clip(&prepared_query, l_secs);
    let query_duration_secs = query_clip.duration_secs();
    let query_fp = deps
        .fingerprinter
        .fingerprint(&query_clip)
        .map_err(|e| AlignmentError::EngineFailed(format!("query fingerprint failed: {e}")))?;

    let dur_a = reference.extent.effective().as_secs_f64();
    if dur_a < l_secs {
        return Ok(ReferenceLocalizationOutcome::skipped(
            "reference shorter than query window",
            0,
            stride_base,
        ));
    }

    // Window cap: widen stride until the estimated window count is within budget.
    let stride = resolve_stride(
        dur_a,
        l_secs,
        stride_base,
        config.alignment.query_max_windows_scored,
    );

    // 2. Coarse ring-buffer sliding window over the reference.
    let candidates = coarse_search(
        reference.session,
        reference.track,
        &query_fp,
        CoarseSearchParams {
            l_secs,
            stride_secs: stride,
            bucket_secs: config.alignment.query_decode_bucket_secs,
        },
        &deps,
        progress,
    )?;
    let windows_scored = candidates.len() as u32;

    // 3. Cluster candidates and select the best anchor.
    let Some((winner, ambiguous)) = select_best_cluster(&candidates, query_duration_secs) else {
        return Ok(ReferenceLocalizationOutcome::skipped(
            "no fingerprint match found in reference",
            windows_scored,
            stride,
        ));
    };

    let confidence = if ambiguous {
        winner.confidence * AMBIGUOUS_CONFIDENCE_FACTOR
    } else {
        winner.confidence
    };
    if confidence < config.alignment.query_min_match_score {
        return Ok(ReferenceLocalizationOutcome::skipped(
            "best localization confidence below threshold",
            windows_scored,
            stride,
        ));
    }

    // 4. PCM-refine the winning anchor (top-1 in Q1).
    let search_radius = (0.1 * query_duration_secs).max(15.0);
    let refined_anchor = refine_query_anchor(
        &query_clip,
        reference.session,
        reference.track,
        QueryAnchorRefine {
            coarse_anchor_ref_secs: winner.anchor_ref_secs,
            query_duration_secs,
            reference_effective_secs: dur_a,
            coarse_confidence: winner.confidence,
            search_radius_secs: search_radius,
        },
        &deps,
        progress,
    );

    Ok(ReferenceLocalizationOutcome {
        anchor_ref_secs: refined_anchor,
        query_duration_secs,
        winning_window_start_secs: winner.window_start_secs,
        winning_window_end_secs: winner.window_end_secs,
        confidence,
        ambiguous,
        windows_scored,
        search_stride_secs: stride,
        skip_reason: None,
    })
}

struct CoarseSearchParams {
    l_secs: f64,
    stride_secs: f64,
    bucket_secs: f64,
}

/// Ring-buffer sliding window. Buckets (`bucket_secs`) feed a length-`L` ring scored every
/// `stride`; memory stays `O(L)`. The callback must not re-enter the session.
fn coarse_search<RS: MediaSession>(
    reference: &mut RS,
    reference_track: &AudioTrack,
    query_fp: &crate::domain::Fingerprint,
    params: CoarseSearchParams,
    deps: &LocateQueryDeps<'_>,
    progress: &dyn ProgressReporter,
) -> Result<Vec<Candidate>, AlignmentError> {
    // The container/probe hint may disagree with the decoded rate (HE-AAC/SBR
    // advertises half the decoder output rate). The ring holds *decoded* samples,
    // so the window length (`l_samples`) and stride must be sized from the decoded
    // rate — sizing them from the hint would collect the wrong span of audio and
    // misplace every candidate (M-HE). Derive both lazily from the first bucket.
    let hint_rate = reference_track.sample_rate;

    let prep = PcmPreparationOptions {
        normalize_loudness: true,
        trim_silence: true,
        window_slide_secs: 0,
    };

    let mut ring: VecDeque<i16> = VecDeque::new();
    let mut ring_start_index: u64 = 0;
    let mut next_score_index: u64 = 0;
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut sample_rate: u32 = hint_rate;
    let mut l_samples: usize = 0;
    let mut stride_samples: u64 = 0;
    let mut window_dims_ready = false;

    reference
        .scan_mono_buckets(
            reference_track,
            params.bucket_secs,
            progress,
            "query-coarse",
            &mut |bucket| {
                sample_rate = bucket.pcm.sample_rate;
                if !window_dims_ready {
                    l_samples = secs_to_samples(params.l_secs, sample_rate);
                    stride_samples = secs_to_samples(params.stride_secs, sample_rate) as u64;
                    if sample_rate != hint_rate {
                        tracing::warn!(
                            container_rate = hint_rate,
                            decoded_rate = sample_rate,
                            "query coarse search: sample-rate hint disagrees with decoded rate; sizing window from decoded rate"
                        );
                    }
                    window_dims_ready = true;
                }
                ring.extend(bucket.pcm.samples.iter().copied());
                trim_front(&mut ring, &mut ring_start_index, next_score_index);

                loop {
                    let window_end_index = next_score_index + l_samples as u64;
                    let ring_end_index = ring_start_index + ring.len() as u64;
                    if window_end_index > ring_end_index {
                        break;
                    }

                    let offset_in_ring = (next_score_index - ring_start_index) as usize;
                    let samples: Vec<i16> = ring
                        .iter()
                        .skip(offset_in_ring)
                        .take(l_samples)
                        .copied()
                        .collect();
                    let window_clip = MonoPcmClip {
                        sample_rate,
                        samples,
                        decode_error_skips: 0,
                        decoded_sample_count: None,
                    };
                    let pos_secs = next_score_index as f64 / f64::from(sample_rate);

                    // Prepare + fingerprint inside the callback only (no session re-entry).
                    match prepare_clip_for_fingerprint(&window_clip, prep) {
                        Ok(prepared) => match deps.fingerprinter.fingerprint(&prepared) {
                            Ok(window_fp) => match deps.aligner.find_offset(&window_fp, query_fp)
                            {
                                Ok(estimate) => {
                                    // anchor = pos - r (Q0-confirmed sign; see § Coarse search).
                                    let anchor = pos_secs - estimate.offset_secs;
                                    candidates.push(Candidate {
                                        anchor_ref_secs: anchor,
                                        confidence: estimate.confidence,
                                        window_start_secs: pos_secs,
                                        window_end_secs: pos_secs + params.l_secs,
                                    });
                                }
                                Err(err) => {
                                    tracing::debug!(
                                        error = %err,
                                        window_start_secs = pos_secs,
                                        "coarse query: aligner.find_offset failed"
                                    );
                                }
                            },
                            Err(err) => {
                                tracing::debug!(
                                    error = %err,
                                    window_start_secs = pos_secs,
                                    "coarse query: fingerprint failed"
                                );
                            }
                        },
                        Err(err) => {
                            tracing::debug!(
                                error = %err,
                                window_start_secs = pos_secs,
                                "coarse query: prepare_clip_for_fingerprint failed"
                            );
                        }
                    }

                    next_score_index += stride_samples;
                    trim_front(&mut ring, &mut ring_start_index, next_score_index);
                }

                Ok(())
            },
        )
        .map_err(|e| AlignmentError::EngineFailed(format!("reference scan failed: {e}")))?;

    Ok(candidates)
}

/// Cluster candidates by anchor (±tolerance) and return the best cluster's representative
/// plus whether a competing cluster makes the localization ambiguous.
///
/// `query_duration_secs` is the minimum anchor separation for ambiguity: a competing cluster
/// *within* the query's footprint is the same match seen through an overlapping (offset) search
/// window — partial-overlap windows report shifted anchors with high confidence — not a genuine
/// alternate location. Genuine repeats sit at a distinct timeline position (≥ query length away).
fn select_best_cluster(
    candidates: &[Candidate],
    query_duration_secs: f64,
) -> Option<(Candidate, bool)> {
    if candidates.is_empty() {
        return None;
    }

    // Each cluster keeps its highest-confidence representative.
    let mut clusters: Vec<Candidate> = Vec::new();
    for cand in candidates {
        if let Some(rep) = clusters
            .iter_mut()
            .find(|rep| (rep.anchor_ref_secs - cand.anchor_ref_secs).abs() <= ANCHOR_CLUSTER_TOLERANCE_SECS)
        {
            if cand.confidence > rep.confidence {
                *rep = *cand;
            }
        } else {
            clusters.push(*cand);
        }
    }

    clusters.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let best = clusters[0];
    // A competing cluster is genuinely ambiguous only when it is both comparably strong AND far
    // enough away to be a distinct location (≥ one query length), not a partial-overlap artifact.
    let min_separation = query_duration_secs.max(ANCHOR_CLUSTER_TOLERANCE_SECS);
    let ambiguous = clusters.iter().skip(1).any(|other| {
        other.confidence >= best.confidence * AMBIGUITY_SCORE_FRACTION
            && (other.anchor_ref_secs - best.anchor_ref_secs).abs() >= min_separation
    });
    Some((best, ambiguous))
}

struct QueryAnchorRefine {
    coarse_anchor_ref_secs: f64,
    query_duration_secs: f64,
    reference_effective_secs: f64,
    coarse_confidence: f32,
    search_radius_secs: f64,
}

/// PCM-refine the coarse anchor. Extracts a reference haystack around the anchor and slides the
/// (prepared) query template across it via [`refine_offset_around_prior`]; returns the refined
/// anchor, or the coarse anchor unchanged if extraction fails or the result drifts past radius.
fn refine_query_anchor<RS: MediaSession>(
    query_clip: &MonoPcmClip,
    reference: &mut RS,
    reference_track: &AudioTrack,
    refine: QueryAnchorRefine,
    deps: &LocateQueryDeps<'_>,
    progress: &dyn ProgressReporter,
) -> f64 {
    let hay_start = (refine.coarse_anchor_ref_secs - refine.search_radius_secs).max(0.0);
    let hay_end = (refine.coarse_anchor_ref_secs
        + refine.query_duration_secs
        + refine.search_radius_secs)
        .min(refine.reference_effective_secs);
    if hay_end - hay_start < refine.query_duration_secs * 0.5 {
        return refine.coarse_anchor_ref_secs;
    }

    let window = ClipWindow::new(
        Duration::from_secs_f64(hay_start),
        Duration::from_secs_f64(hay_end),
        ClipLabel::Interior,
    );
    let haystack = match reference.extract_mono(reference_track, &window, progress, "query-refine") {
        Ok(clip) => clip,
        Err(_) => return refine.coarse_anchor_ref_secs,
    };
    if haystack.sample_rate != query_clip.sample_rate {
        return refine.coarse_anchor_ref_secs;
    }

    // left = query template, right = haystack. prior: haystack_local = query_local + offset,
    // so offset = coarse_anchor - hay_start; refined anchor = hay_start + refined.offset.
    let prior = ClipMatchEstimate {
        offset_secs: refine.coarse_anchor_ref_secs - hay_start,
        confidence: refine.coarse_confidence.max(0.1),
    };
    let refined = refine_offset_around_prior(
        query_clip,
        &haystack,
        prior,
        refine.search_radius_secs,
        deps.resampler,
        deps.correlator,
    );
    let refined_anchor = hay_start + refined.offset_secs;
    if (refined_anchor - refine.coarse_anchor_ref_secs).abs() > refine.search_radius_secs {
        return refine.coarse_anchor_ref_secs;
    }
    refined_anchor
}

/// Widen stride (×2) until estimated window count fits the budget.
fn resolve_stride(dur_a: f64, l_secs: f64, stride_base: f64, max_windows: u32) -> f64 {
    if max_windows == 0 || stride_base <= 0.0 {
        return stride_base;
    }
    let mut stride = stride_base;
    while estimated_windows(dur_a, l_secs, stride) > max_windows {
        stride *= 2.0;
    }
    if stride > stride_base {
        warn!(
            stride_base,
            stride, max_windows, "query coarse search widened stride to respect window cap"
        );
    }
    stride
}

fn estimated_windows(dur_a: f64, l_secs: f64, stride: f64) -> u32 {
    if stride <= 0.0 {
        return u32::MAX;
    }
    (((dur_a - l_secs) / stride).ceil() as i64 + 1).clamp(0, i64::from(u32::MAX)) as u32
}

fn truncate_clip(clip: &MonoPcmClip, max_secs: f64) -> MonoPcmClip {
    let max_samples = secs_to_samples(max_secs, clip.sample_rate);
    if clip.samples.len() <= max_samples {
        return clip.clone();
    }
    MonoPcmClip {
        sample_rate: clip.sample_rate,
        samples: clip.samples[..max_samples].to_vec(),
        decode_error_skips: clip.decode_error_skips,
        decoded_sample_count: None,
    }
}

fn secs_to_samples(secs: f64, rate: u32) -> usize {
    (secs * f64::from(rate)).round().max(0.0) as usize
}

fn trim_front(ring: &mut VecDeque<i16>, ring_start_index: &mut u64, keep_from: u64) {
    while *ring_start_index < keep_from && !ring.is_empty() {
        ring.pop_front();
        *ring_start_index += 1;
    }
}

#[cfg(test)]
mod tests;
