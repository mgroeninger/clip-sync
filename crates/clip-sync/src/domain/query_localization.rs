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

/// Result of searching a short query clip against a long reference timeline.
///
/// `PartialEq` is derived for test convenience; float fields are not semantically exact.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryLocalization {
    /// Reference (A) timeline position where query (B) `t = 0` aligns.
    pub anchor_a_secs: f64,
    /// Same as `mapped_region.video_a_start_secs` — explicit alias for human-oriented output.
    pub clip_on_a_start_secs: f64,
    /// Same as `mapped_region.video_a_end_secs`.
    pub clip_on_a_end_secs: f64,
    /// Same as `mapped_region.video_b_start_secs` (usually 0).
    pub clip_on_b_start_secs: f64,
    /// Same as `mapped_region.video_b_end_secs`.
    pub clip_on_b_end_secs: f64,
    /// Shared region implied by the anchor + query duration.
    pub mapped_region: TimelineOverlap,
    /// Coarse search stride actually used (may widen if the window cap was hit).
    pub search_stride_secs: f64,
    /// Reference (A) timeline bounds of the winning coarse window.
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
    /// Build a localization from a chosen anchor + measured coarse-search facts, deriving the
    /// mapped region (and its human-oriented aliases) from the file extents.
    #[allow(clippy::too_many_arguments)]
    pub fn from_anchor(
        anchor_a_secs: f64,
        query_duration_secs: f64,
        extent_a: MediaExtent,
        extent_b: MediaExtent,
        confidence: f32,
        ambiguous: bool,
        search_stride_secs: f64,
        winning_window_start_secs: f64,
        winning_window_end_secs: f64,
        windows_scored: u32,
    ) -> Self {
        let mapped_region =
            compute_mapped_region(anchor_a_secs, query_duration_secs, extent_a, extent_b);
        Self {
            anchor_a_secs,
            clip_on_a_start_secs: mapped_region.video_a_start_secs,
            clip_on_a_end_secs: mapped_region.video_a_end_secs,
            clip_on_b_start_secs: mapped_region.video_b_start_secs,
            clip_on_b_end_secs: mapped_region.video_b_end_secs,
            mapped_region,
            search_stride_secs,
            winning_window_start_secs,
            winning_window_end_secs,
            confidence,
            ambiguous,
            windows_scored,
            skip_reason: None,
        }
    }

    /// A localization carrying no recommendation (no match / skipped), with a reason.
    pub fn skipped(reason: impl Into<String>, windows_scored: u32, search_stride_secs: f64) -> Self {
        let empty = TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 0.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 0.0,
            shared_length_secs: 0.0,
        };
        Self {
            anchor_a_secs: 0.0,
            clip_on_a_start_secs: 0.0,
            clip_on_a_end_secs: 0.0,
            clip_on_b_start_secs: 0.0,
            clip_on_b_end_secs: 0.0,
            mapped_region: empty,
            search_stride_secs,
            winning_window_start_secs: 0.0,
            winning_window_end_secs: 0.0,
            confidence: 0.0,
            ambiguous: false,
            windows_scored,
            skip_reason: Some(reason.into()),
        }
    }

    /// `recommended_offset_secs = -anchor_a_secs` (seconds to add to A to align with B).
    pub fn recommended_offset_secs(&self) -> Option<f64> {
        if self.skip_reason.is_some() {
            None
        } else {
            Some(-self.anchor_a_secs)
        }
    }
}

/// Build an [`AlignmentResult`] from a query-reference localization.
///
/// Query mode emits a single synthetic `Start` `ClipMatch` describing the winning search window
/// on A, so `start_clip()` headline selection still works. A skipped/no-match localization
/// yields an un-aligned result (no recommended offset) so callers may still scan A.
pub fn build_query_alignment_result(
    localization: QueryLocalization,
    min_match_score: f32,
) -> AlignmentResult {
    let recommended_offset_secs = localization.recommended_offset_secs();
    let aligned = recommended_offset_secs.is_some() && localization.confidence >= min_match_score;

    let clip = ClipMatch {
        label: ClipLabel::Start,
        window_start_secs: localization.winning_window_start_secs,
        window_end_secs: localization.winning_window_end_secs,
        aligned,
        offset_secs: aligned.then_some(recommended_offset_secs).flatten(),
        confidence: localization.confidence,
        video_a_decode_skips: 0,
        video_b_decode_skips: 0,
        repetition: None,
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
    }
}

/// Shared region implied by `anchor_a` + `query_duration`, clamped to each file's
/// [`MediaExtent::effective`] (not raw container duration).
///
/// Mirrors the `b = a + offset` convention with `offset = -anchor_a_secs`. The A low end is
/// clamped to `0` (negative-anchor convention — matches `holdout_window_candidates`).
pub fn compute_mapped_region(
    anchor_a_secs: f64,
    query_duration_secs: f64,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
) -> TimelineOverlap {
    let a_eff = extent_a.effective().as_secs_f64();
    let b_eff = extent_b.effective().as_secs_f64();
    let offset = -anchor_a_secs;

    let t_lo = anchor_a_secs.max(0.0);
    // Upper bound: query end, clamped to A's extent and to B's extent expressed in A-space.
    let t_hi = (anchor_a_secs + query_duration_secs)
        .min(a_eff)
        .min(b_eff + anchor_a_secs);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn extent(secs: f64) -> MediaExtent {
        MediaExtent::from_declared(Duration::from_secs_f64(secs))
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
        let extent_a = MediaExtent::new(
            Duration::from_secs(3600),
            Some(Duration::from_secs(3000)),
        );
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
    fn from_anchor_populates_aliases_from_mapped_region() {
        let loc = QueryLocalization::from_anchor(
            2700.0, 480.0, extent(3600.0), extent(480.0), 0.91, false, 60.0, 2640.0, 3120.0, 60,
        );
        assert_eq!(loc.clip_on_a_start_secs, loc.mapped_region.video_a_start_secs);
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
        let loc = QueryLocalization::from_anchor(
            2700.0, 480.0, extent(3600.0), extent(480.0), 0.91, false, 60.0, 2640.0, 3120.0, 60,
        );
        let result = build_query_alignment_result(loc, 0.3);

        assert_eq!(result.alignment_mode_used, Some(AlignmentModeUsed::QueryReference));
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
        assert_eq!(result.alignment_mode_used, Some(AlignmentModeUsed::QueryReference));
        assert!(result
            .query_localization
            .as_ref()
            .and_then(|q| q.skip_reason.as_deref())
            .is_some());
    }

    #[test]
    fn build_result_below_min_match_score_is_unaligned() {
        let loc = QueryLocalization::from_anchor(
            100.0, 90.0, extent(1000.0), extent(90.0), 0.2, false, 60.0, 40.0, 130.0, 16,
        );
        // Confidence 0.2 < min_match_score 0.3 → not aligned even with an anchor.
        let result = build_query_alignment_result(loc, 0.3);
        assert!(!result.start_aligned);
        assert_eq!(result.recommended_offset_secs, None);
    }
}
