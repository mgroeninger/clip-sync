//! Query-reference localization domain types: the result of searching a short *query* clip
//! against a long *reference* timeline. No serde here (layer purity) — JSON DTOs live in
//! `application::report`.

use crate::domain::alignment::{AlignmentResult, ClipMatch, TimelineOverlap};
use crate::domain::clip_window::ClipLabel;
use crate::domain::media_extent::MediaExtent;

/// How an alignment run actually chose its algorithm (recorded on the result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentModeUsed {
    Symmetric,
    QueryReference,
}

/// Orientation-neutral result of sliding a short query across a long reference timeline.
///
/// All timeline positions are on the **reference** (longer) file. Callers map this into
/// A/B-framed [`QueryLocalization`] via [`QueryLocalization::from_reference_outcome`].
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLocalizationOutcome {
    /// Reference-timeline position where the query clip's `t = 0` aligns.
    pub anchor_ref_secs: f64,
    /// Prepared query duration used for mapping (seconds).
    pub query_duration_secs: f64,
    /// Reference-timeline bounds of the winning coarse search window.
    pub winning_window_start_secs: f64,
    pub winning_window_end_secs: f64,
    /// Localization confidence (coarse Chromaprint cluster; ×0.5 when ambiguous).
    pub confidence: f32,
    /// True when a competing anchor cluster scored comparably (repeated content).
    pub ambiguous: bool,
    /// Coarse windows fingerprinted (after any stride widening).
    pub windows_scored: u32,
    /// Coarse search stride actually used (may widen if the window cap was hit).
    pub search_stride_secs: f64,
    /// Set when search produced no usable localization (and why).
    pub skip_reason: Option<String>,
}

impl ReferenceLocalizationOutcome {
    /// A search outcome carrying no anchor (no match / skipped), with a reason.
    pub fn skipped(
        reason: impl Into<String>,
        windows_scored: u32,
        search_stride_secs: f64,
    ) -> Self {
        Self {
            anchor_ref_secs: 0.0,
            query_duration_secs: 0.0,
            winning_window_start_secs: 0.0,
            winning_window_end_secs: 0.0,
            confidence: 0.0,
            ambiguous: false,
            windows_scored,
            search_stride_secs,
            skip_reason: Some(reason.into()),
        }
    }
}

/// Result of searching a short query clip against a long reference timeline.
///
/// `mapped_region` is the single source of truth for placement; `clip_on_*` and
/// `anchor_ref_secs` are derived in [`QueryLocalization::from_reference_outcome`].
/// `PartialEq` is derived for test convenience; float fields are not semantically exact.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryLocalization {
    /// Position on the **longer (reference)** file where the short clip's `t = 0` aligns
    /// (= `mapped_region.video_a_start` when A is reference, else `video_b_start`).
    pub anchor_ref_secs: f64,
    /// Same as `mapped_region.video_a_start_secs` — explicit alias for human-oriented output.
    pub clip_on_a_start_secs: f64,
    /// Same as `mapped_region.video_a_end_secs`.
    pub clip_on_a_end_secs: f64,
    /// Same as `mapped_region.video_b_start_secs` (usually 0 when B is the query).
    pub clip_on_b_start_secs: f64,
    /// Same as `mapped_region.video_b_end_secs`.
    pub clip_on_b_end_secs: f64,
    /// Shared region implied by the anchor + query duration (always A/B-oriented).
    pub mapped_region: TimelineOverlap,
    /// Seconds to add to A's timeline to align with B (`b = a + offset`). `None` when skipped.
    pub recommended_offset_secs: Option<f64>,
    /// Coarse search stride actually used (may widen if the window cap was hit).
    pub search_stride_secs: f64,
    /// Reference-timeline bounds of the winning coarse window (longer file).
    pub winning_window_start_secs: f64,
    pub winning_window_end_secs: f64,
    /// Localization confidence (coarse Chromaprint cluster; ×0.5 when ambiguous).
    pub confidence: f32,
    /// True when a competing anchor cluster scored comparably (repeated content).
    pub ambiguous: bool,
    /// Coarse windows fingerprinted (after any stride widening).
    pub windows_scored: u32,
    /// Set when query mode produced no usable localization (and why).
    pub skip_reason: Option<String>,
}

impl QueryLocalization {
    /// Build localization when A is the reference (longer) file.
    pub fn from_a_reference_outcome(
        outcome: ReferenceLocalizationOutcome,
        extent_a: MediaExtent,
        extent_b: MediaExtent,
    ) -> Self {
        Self::from_reference_outcome(outcome, true, extent_a, extent_b)
    }

    /// Map an orientation-neutral search outcome into A/B repair roles.
    pub fn from_reference_outcome(
        outcome: ReferenceLocalizationOutcome,
        reference_is_a: bool,
        extent_a: MediaExtent,
        extent_b: MediaExtent,
    ) -> Self {
        if let Some(reason) = outcome.skip_reason {
            return Self::skipped(reason, outcome.windows_scored, outcome.search_stride_secs);
        }

        debug_assert!(
            reference_is_a == (extent_a.effective() >= extent_b.effective()),
            "reference_is_a={reference_is_a} inconsistent with effective durations \
             (A={:?}, B={:?})",
            extent_a.effective(),
            extent_b.effective(),
        );

        let anchor = outcome.anchor_ref_secs;
        let mapped_region = if reference_is_a {
            compute_mapped_region(anchor, outcome.query_duration_secs, extent_a, extent_b)
        } else {
            swap_timeline_overlap_a_b(compute_mapped_region(
                anchor,
                outcome.query_duration_secs,
                extent_b,
                extent_a,
            ))
        };

        let recommended_offset_secs = Some(if reference_is_a { -anchor } else { anchor });
        let anchor_ref_secs = if reference_is_a {
            mapped_region.video_a_start_secs
        } else {
            mapped_region.video_b_start_secs
        };

        Self {
            anchor_ref_secs,
            clip_on_a_start_secs: mapped_region.video_a_start_secs,
            clip_on_a_end_secs: mapped_region.video_a_end_secs,
            clip_on_b_start_secs: mapped_region.video_b_start_secs,
            clip_on_b_end_secs: mapped_region.video_b_end_secs,
            mapped_region,
            recommended_offset_secs,
            search_stride_secs: outcome.search_stride_secs,
            winning_window_start_secs: outcome.winning_window_start_secs,
            winning_window_end_secs: outcome.winning_window_end_secs,
            confidence: outcome.confidence,
            ambiguous: outcome.ambiguous,
            windows_scored: outcome.windows_scored,
            skip_reason: None,
        }
    }

    /// A localization carrying no recommendation (no match / skipped), with a reason.
    pub fn skipped(
        reason: impl Into<String>,
        windows_scored: u32,
        search_stride_secs: f64,
    ) -> Self {
        let empty = TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 0.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 0.0,
            shared_length_secs: 0.0,
        };
        Self {
            anchor_ref_secs: 0.0,
            clip_on_a_start_secs: 0.0,
            clip_on_a_end_secs: 0.0,
            clip_on_b_start_secs: 0.0,
            clip_on_b_end_secs: 0.0,
            mapped_region: empty,
            recommended_offset_secs: None,
            search_stride_secs,
            winning_window_start_secs: 0.0,
            winning_window_end_secs: 0.0,
            confidence: 0.0,
            ambiguous: false,
            windows_scored,
            skip_reason: Some(reason.into()),
        }
    }

    /// Stored offset (seconds to add to A to align with B). `None` when localization was skipped.
    pub fn recommended_offset_secs(&self) -> Option<f64> {
        self.recommended_offset_secs
    }
}

/// True when A is the longer (reference) file in query-reference mode.
pub fn query_reference_is_a(loc: &QueryLocalization) -> bool {
    (loc.anchor_ref_secs - loc.mapped_region.video_a_start_secs).abs() < 1e-6
}

/// Assert stored offset matches orientation: `-anchor_ref` when A is reference, `+anchor_ref` when B is.
#[cfg(test)]
pub fn assert_recommended_offset_matches_orientation(loc: &QueryLocalization, tolerance_secs: f64) {
    let offset = loc
        .recommended_offset_secs()
        .expect("expected recommended offset");
    let expected = if query_reference_is_a(loc) {
        -loc.anchor_ref_secs
    } else {
        loc.anchor_ref_secs
    };
    assert!(
        (offset - expected).abs() <= tolerance_secs,
        "offset {offset} expected {expected} ± {tolerance_secs} (anchor_ref={})",
        loc.anchor_ref_secs
    );
}

/// Winning coarse-search window rebased onto **A's** timeline for discovery/hold-out placement.
///
/// [`QueryLocalization::winning_window_*`] stay on the reference (longer) file; synthetic
/// `ClipMatch` windows and `discovery_windows` use this helper so high-rate/verify see A time.
pub fn winning_window_on_a_timeline(loc: &QueryLocalization) -> (f64, f64) {
    let ws = loc.winning_window_start_secs;
    let we = loc.winning_window_end_secs.max(ws);
    if query_reference_is_a(loc) {
        return (ws, we);
    }
    let offset = loc.recommended_offset_secs.unwrap_or(0.0);
    let a_start = (ws - offset).max(0.0);
    let a_end = (we - offset).max(a_start);
    (a_start, a_end)
}

/// Build an [`AlignmentResult`] from a query-reference localization.
///
/// Query mode emits a single synthetic `Start` `ClipMatch` describing the winning search window
/// on **A's** timeline (rebased from the reference timeline when B is longer), so `start_clip()`
/// and hold-out placement match the symmetric path. A skipped/no-match localization yields an
/// un-aligned result (no recommended offset) so callers may still scan A.
pub fn build_query_alignment_result(
    localization: QueryLocalization,
    min_match_score: f32,
) -> AlignmentResult {
    let recommended_offset_secs = localization.recommended_offset_secs();
    let aligned = recommended_offset_secs.is_some() && localization.confidence >= min_match_score;
    let (window_start_secs, window_end_secs) = winning_window_on_a_timeline(&localization);

    let clip = ClipMatch {
        label: ClipLabel::Start,
        window_start_secs,
        window_end_secs,
        aligned,
        offset_secs: aligned.then_some(recommended_offset_secs).flatten(),
        confidence: localization.confidence,
        video_a_decode_skips: 0,
        video_b_decode_skips: 0,
        repetition: None,
        video_b_window_start_secs: None,
        video_b_window_end_secs: None,
    };

    let start_overlap = aligned.then_some(localization.mapped_region);

    AlignmentResult {
        clips: vec![clip],
        start_aligned: aligned,
        end_aligned: None,
        recommended_offset_secs: aligned.then_some(recommended_offset_secs).flatten(),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: Some(AlignmentModeUsed::QueryReference),
        query_localization: Some(localization),
        end_clip_anchor: None,
    }
}

/// Shared region implied by reference anchor + `query_duration`, clamped to each file's
/// [`MediaExtent::effective`] (not raw container duration).
///
/// `anchor_ref_secs` is on the **reference** (first extent) timeline. Mirrors the
/// `b = a + offset` convention with `offset = -anchor_ref_secs` when the reference is A.
/// The A low end is clamped to `0` (negative-anchor convention — matches `holdout_window_candidates`).
pub fn compute_mapped_region(
    anchor_ref_secs: f64,
    query_duration_secs: f64,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
) -> TimelineOverlap {
    let a_eff = extent_a.effective().as_secs_f64();
    let b_eff = extent_b.effective().as_secs_f64();
    let offset = -anchor_ref_secs;

    let t_lo = anchor_ref_secs.max(0.0);
    // Upper bound: query end, clamped to A's extent and to B's extent expressed in A-space.
    let t_hi = (anchor_ref_secs + query_duration_secs)
        .min(a_eff)
        .min(b_eff + anchor_ref_secs);

    if t_hi <= t_lo {
        let b_edge = (t_lo + offset).max(0.0);
        return TimelineOverlap {
            video_a_start_secs: t_lo,
            video_a_end_secs: t_lo,
            video_b_start_secs: b_edge,
            video_b_end_secs: b_edge,
            shared_length_secs: 0.0,
        };
    }

    TimelineOverlap {
        video_a_start_secs: t_lo,
        video_a_end_secs: t_hi,
        video_b_start_secs: t_lo + offset,
        video_b_end_secs: t_hi + offset,
        shared_length_secs: t_hi - t_lo,
    }
}

/// Exchange A/B spans when remapping a reference-first region into repair roles.
fn swap_timeline_overlap_a_b(overlap: TimelineOverlap) -> TimelineOverlap {
    TimelineOverlap {
        video_a_start_secs: overlap.video_b_start_secs,
        video_a_end_secs: overlap.video_b_end_secs,
        video_b_start_secs: overlap.video_a_start_secs,
        video_b_end_secs: overlap.video_a_end_secs,
        shared_length_secs: overlap.shared_length_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn extent(secs: f64) -> MediaExtent {
        MediaExtent::from_declared(Duration::from_secs_f64(secs))
    }

    fn sample_outcome(anchor: f64) -> ReferenceLocalizationOutcome {
        ReferenceLocalizationOutcome {
            anchor_ref_secs: anchor,
            query_duration_secs: 480.0,
            winning_window_start_secs: anchor - 60.0,
            winning_window_end_secs: anchor + 420.0,
            confidence: 0.91,
            ambiguous: false,
            windows_scored: 60,
            search_stride_secs: 60.0,
            skip_reason: None,
        }
    }

    fn assert_b_equals_a_plus_offset(loc: &QueryLocalization) {
        let offset = loc
            .recommended_offset_secs()
            .expect("expected stored offset");
        let a = loc.mapped_region.video_a_start_secs;
        let b = loc.mapped_region.video_b_start_secs;
        assert!(
            (b - (a + offset)).abs() < 1e-9,
            "b = a + offset failed: a={a}, b={b}, offset={offset}"
        );
    }

    #[test]
    fn from_reference_outcome_a_reference_maps_anchor() {
        let outcome = sample_outcome(2700.0);
        let loc =
            QueryLocalization::from_reference_outcome(outcome, true, extent(3600.0), extent(480.0));
        assert_eq!(loc.recommended_offset_secs(), Some(-2700.0));
        assert_b_equals_a_plus_offset(&loc);
    }

    #[test]
    fn from_reference_outcome_b_reference_positive_offset_and_spans() {
        let loc = QueryLocalization::from_reference_outcome(
            sample_outcome(2700.0),
            false,
            extent(480.0),
            extent(3600.0),
        );
        assert_eq!(loc.recommended_offset_secs(), Some(2700.0));
        assert!((loc.mapped_region.video_a_start_secs - 0.0).abs() < 1e-9);
        assert!((loc.mapped_region.video_a_end_secs - 480.0).abs() < 1e-9);
        assert!((loc.mapped_region.video_b_start_secs - 2700.0).abs() < 1e-9);
        assert!((loc.mapped_region.video_b_end_secs - 3180.0).abs() < 1e-9);
        assert!((loc.anchor_ref_secs - 2700.0).abs() < 1e-9);
        assert_b_equals_a_plus_offset(&loc);
    }

    #[test]
    fn from_reference_outcome_skipped_matches_skipped() {
        let outcome = ReferenceLocalizationOutcome::skipped("no match", 40, 60.0);
        let loc =
            QueryLocalization::from_reference_outcome(outcome, true, extent(3600.0), extent(480.0));
        assert_eq!(loc.skip_reason.as_deref(), Some("no match"));
        assert_eq!(loc.recommended_offset_secs(), None);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "reference_is_a")]
    fn from_reference_outcome_debug_asserts_when_reference_is_a_wrong() {
        let outcome = sample_outcome(90.0);
        let _ =
            QueryLocalization::from_reference_outcome(outcome, true, extent(70.0), extent(180.0));
    }

    #[test]
    fn from_reference_outcome_b_reference_negative_anchor_clamps_a_low() {
        let outcome = ReferenceLocalizationOutcome {
            anchor_ref_secs: -5.0,
            query_duration_secs: 100.0,
            winning_window_start_secs: -65.0,
            winning_window_end_secs: 415.0,
            confidence: 0.88,
            ambiguous: false,
            windows_scored: 20,
            search_stride_secs: 60.0,
            skip_reason: None,
        };
        let loc = QueryLocalization::from_reference_outcome(
            outcome,
            false,
            extent(100.0),
            extent(1000.0),
        );
        assert_eq!(loc.recommended_offset_secs(), Some(-5.0));
        assert!((loc.mapped_region.video_a_start_secs - 5.0).abs() < 1e-9);
        assert!((loc.mapped_region.video_b_start_secs - 0.0).abs() < 1e-9);
        assert_b_equals_a_plus_offset(&loc);
        let (a_ws, _) = winning_window_on_a_timeline(&loc);
        assert!(
            (a_ws - 0.0).abs() < 1e-9,
            "reference window on B rebased to A should clamp at 0, got {a_ws}"
        );
    }

    #[test]
    fn winning_window_on_a_timeline_b_reference_mid_anchor() {
        let loc = QueryLocalization::from_reference_outcome(
            sample_outcome(2700.0),
            false,
            extent(480.0),
            extent(3600.0),
        );
        let (a_ws, a_we) = winning_window_on_a_timeline(&loc);
        assert!((a_ws - 0.0).abs() < 1e-9);
        assert!((a_we - 420.0).abs() < 1e-9);
        let result = build_query_alignment_result(loc, 0.3);
        let start = result.start_clip().expect("synthetic start");
        assert!((start.window_start_secs - 0.0).abs() < 1e-9);
        assert!((start.window_end_secs - 420.0).abs() < 1e-9);
    }

    #[test]
    fn mapped_region_mid_timeline_anchor() {
        let region = compute_mapped_region(2700.0, 480.0, extent(3600.0), extent(480.0));
        assert!((region.video_a_start_secs - 2700.0).abs() < 1e-9);
        assert!((region.video_a_end_secs - 3180.0).abs() < 1e-9);
        assert!((region.video_b_start_secs - 0.0).abs() < 1e-9);
        assert!((region.video_b_end_secs - 480.0).abs() < 1e-9);
        assert!((region.shared_length_secs - 480.0).abs() < 1e-9);
    }

    #[test]
    fn mapped_region_clamps_to_effective_duration() {
        // Decodable extent shorter than declared clips the A end.
        let extent_a = MediaExtent::new(Duration::from_secs(3600), Some(Duration::from_secs(3000)));
        let region = compute_mapped_region(2700.0, 480.0, extent_a, extent(480.0));
        assert!((region.video_a_end_secs - 3000.0).abs() < 1e-9);
        // B end follows in B-space: 3000 - 2700 = 300.
        assert!((region.video_b_end_secs - 300.0).abs() < 1e-9);
        assert!((region.shared_length_secs - 300.0).abs() < 1e-9);
    }

    #[test]
    fn mapped_region_negative_anchor_clamps_a_low_end_to_zero() {
        let region = compute_mapped_region(-5.0, 100.0, extent(1000.0), extent(100.0));
        assert!(region.video_a_start_secs >= 0.0);
        assert!((region.video_a_start_secs - 0.0).abs() < 1e-9);
    }

    #[test]
    fn from_a_reference_outcome_populates_aliases_from_mapped_region() {
        let loc = QueryLocalization::from_a_reference_outcome(
            sample_outcome(2700.0),
            extent(3600.0),
            extent(480.0),
        );
        assert_eq!(
            loc.clip_on_a_start_secs,
            loc.mapped_region.video_a_start_secs
        );
        assert_eq!(loc.clip_on_a_end_secs, loc.mapped_region.video_a_end_secs);
        assert_eq!(loc.clip_on_b_end_secs, loc.mapped_region.video_b_end_secs);
        assert_eq!(loc.recommended_offset_secs(), Some(-2700.0));
    }

    #[test]
    fn skipped_localization_has_no_offset() {
        let loc = QueryLocalization::skipped("below threshold", 12, 60.0);
        assert_eq!(loc.recommended_offset_secs(), None);
        assert_eq!(loc.skip_reason.as_deref(), Some("below threshold"));
    }

    #[test]
    fn build_result_aligned_emits_synthetic_start_clip() {
        let loc = QueryLocalization::from_a_reference_outcome(
            sample_outcome(2700.0),
            extent(3600.0),
            extent(480.0),
        );
        let result = build_query_alignment_result(loc, 0.3);

        assert_eq!(
            result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference)
        );
        assert!(result.start_aligned);
        assert_eq!(result.recommended_offset_secs, Some(-2700.0));
        let start = result.start_clip().expect("synthetic start clip");
        assert_eq!(start.window_start_secs, 2640.0);
        assert_eq!(start.offset_secs, Some(-2700.0));
        assert!(result.start_overlap.is_some());
        assert!(result.query_localization.is_some());
    }

    #[test]
    fn build_result_skipped_is_unaligned_but_carries_localization() {
        let loc = QueryLocalization::skipped("no match", 40, 60.0);
        let result = build_query_alignment_result(loc, 0.3);

        assert!(!result.start_aligned);
        assert_eq!(result.recommended_offset_secs, None);
        assert!(result.start_overlap.is_none());
        // Still records that query mode ran (for reporting) + the skip reason.
        assert_eq!(
            result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference)
        );
        assert!(result
            .query_localization
            .as_ref()
            .and_then(|q| q.skip_reason.as_deref())
            .is_some());
    }

    #[test]
    fn build_result_below_min_match_score_is_unaligned() {
        let loc = QueryLocalization::from_a_reference_outcome(
            ReferenceLocalizationOutcome {
                anchor_ref_secs: 100.0,
                query_duration_secs: 90.0,
                winning_window_start_secs: 40.0,
                winning_window_end_secs: 130.0,
                confidence: 0.2,
                ambiguous: false,
                windows_scored: 16,
                search_stride_secs: 60.0,
                skip_reason: None,
            },
            extent(1000.0),
            extent(90.0),
        );
        // Confidence 0.2 < min_match_score 0.3 → not aligned even with an anchor.
        let result = build_query_alignment_result(loc, 0.3);
        assert!(!result.start_aligned);
        assert_eq!(result.recommended_offset_secs, None);
    }
}
