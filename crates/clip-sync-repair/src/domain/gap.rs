use std::path::PathBuf;

use serde::Serialize;

use crate::domain::{AudioTimelineSkew, ScanAlignment};

use crate::domain::gap_equivalence::GapEquivalenceVerdict;
use crate::domain::track_match::TrackCompatibility;

/// Diagnostic comparison of the Chromaprint alignment offset vs the silence-structure-derived
/// offset produced by the R3 bidirectional scan cross-check.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapOffsetAgreement {
    pub silence_based_offset_secs: f64,
    pub alignment_offset_secs: f64,
    /// Absolute difference between the two estimates.
    pub delta_secs: f64,
    /// `true` when `delta_secs` is within the configured tolerance.
    pub agrees: bool,
}

/// A silent region detected in video A's timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    /// Corresponding position in video B, mapped via `recommended_offset_secs`.
    /// `None` when alignment failed or produced no offset — B cannot be probed.
    pub video_b_start_secs: Option<f64>,
    pub video_b_end_secs: Option<f64>,
    /// Whether video B has audio energy at this position (potential fill source).
    pub b_has_energy: bool,
}

impl Gap {
    pub fn duration_secs(&self) -> f64 {
        self.video_a_end_secs - self.video_a_start_secs
    }

    pub fn is_fillable(&self) -> bool {
        self.video_b_start_secs.is_some() && self.b_has_energy
    }

    /// The gap's mapped B span, or `None` when it is absent, degenerate, or negative.
    ///
    /// The single guarded way to read B coordinates off a gap. Rejects the same shapes
    /// [`b_range_fully_scanned`](crate::domain::cross_check::b_range_fully_scanned) rejects —
    /// the two predicates must not drift. In particular a half-mapped gap (start but no end)
    /// is `None` rather than a start on B's timeline paired with an end on A's, and a negative
    /// mapped start is `None` rather than something a caller clamps in one expression and
    /// reports unclamped in the next.
    pub fn mapped_b_span(&self) -> Option<(f64, f64)> {
        match (self.video_b_start_secs, self.video_b_end_secs) {
            (Some(start), Some(end)) if start < end && start >= 0.0 => Some((start, end)),
            _ => None,
        }
    }

    /// Operator-facing reason when [`is_fillable`](Self::is_fillable) is false.
    pub fn unfillable_label(&self) -> &'static str {
        if self.video_b_start_secs.is_some() {
            // Mapped donor is silent — shared pause / nothing to copy (not a missing alignment).
            "both sides silent"
        } else {
            "unmapped"
        }
    }
}

/// True when `[start_secs, end_secs]` lies entirely inside a timeline window.
///
/// Used by fill planning and silence cross-check gating.
pub fn interval_fully_within_window(
    start_secs: f64,
    end_secs: f64,
    window_start_secs: f64,
    window_end_secs: f64,
) -> bool {
    start_secs >= window_start_secs && end_secs <= window_end_secs
}

/// Full gap scan report produced by `ScanGaps`.
#[derive(Debug, Clone)]
pub struct GapReport {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    /// Audio track comparison (channels/rate). `None` when B could not be opened or has no
    /// decodable track — the scan still reports A's gaps.
    pub track_compatibility: Option<TrackCompatibility>,
    pub alignment: ScanAlignment,
    pub gaps: Vec<Gap>,
    /// Per-gap silence-character classification (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate), **index-parallel to
    /// `gaps`**. Always populated by the scan (additive/advisory); empty on reports built before the gate or
    /// by test/legacy constructors. Consumed by `build_gap_fill_plan` only when `skip_equivalent_gaps` is set.
    pub gap_equivalence: Vec<GapEquivalenceVerdict>,
    /// Present when `scan_both` was enabled and both A and B had silence intervals to compare.
    pub gap_offset_agreement: Option<GapOffsetAgreement>,
    /// Decode chunk size used during sequential scan (seconds).
    pub decode_chunk_secs: u64,
    /// Analysis block size used for silence-run detection (milliseconds).
    pub scan_block_ms: u64,
    pub silence_peak_fraction: f32,
    /// When query-reference alignment is used, only gaps inside the mapped clip coverage are fillable.
    pub limit_fill_to_mapped_region: bool,
    /// How far the B silence/level scan progressed on B's native timeline (seconds). `None` when B
    /// was not scanned. Gaps whose mapped core extends past this were not reviewed for donor
    /// occupancy (`b_has_energy` is fail-closed).
    pub b_scanned_end_secs: Option<f64>,
    /// `true` when the B walk aborted mid-file (decode/seek error). Continue is intentional
    /// (report-only safe); callers should surface the truncation timestamp so users know later
    /// mapped gaps were not reviewed.
    pub b_scan_truncated: bool,
    /// Maximum |PTS − sample-clock| observed during gap scan on video A, when measurable.
    pub audio_timeline_skew: Option<AudioTimelineSkew>,
}

impl GapReport {
    /// Gaps where B has mapped audio energy (ignores track layout and mapped-region gate).
    pub fn fillable_count(&self) -> usize {
        self.gaps.iter().filter(|g| g.is_fillable()).count()
    }

    /// Whether the selected A/B track layout allows any splice fill.
    pub fn patch_allowed(&self) -> bool {
        matches!(
            self.track_compatibility.as_ref().map(|tc| tc.verdict),
            Some(super::track_match::CompatibilityVerdict::Identical)
                | Some(super::track_match::CompatibilityVerdict::Compatible)
        )
    }

    /// Gaps that would be included in a fill plan (B energy + patch-allowed tracks + region gate).
    pub fn repairable_count(&self) -> usize {
        if !self.patch_allowed() {
            return 0;
        }
        self.gaps
            .iter()
            .filter(|g| self.is_gap_repairable(g))
            .count()
    }

    /// True when a gap is not **fully** covered by the query-reference mapped region on A.
    ///
    /// Full containment is required by design: B only has audio for the mapped region, so a gap
    /// that straddles a region boundary is only partly covered. Filling it would splice
    /// uncovered audio (silence / out-of-range B) into the exposed part, so such gaps are
    /// conservatively excluded rather than partially filled. Returns `false` (not outside) in
    /// symmetric mode or when no overlap was computed — the gate only applies to query mode.
    pub fn gap_outside_reference_coverage(&self, gap: &Gap) -> bool {
        if !self.alignment.query_reference_mode {
            return false;
        }
        let Some(overlap) = &self.alignment.start_overlap else {
            return false;
        };
        !interval_fully_within_window(
            gap.video_a_start_secs,
            gap.video_a_end_secs,
            overlap.video_a_start_secs,
            overlap.video_a_end_secs,
        )
    }

    /// The equivalence verdict for the gap at `index` (index-parallel to `gaps`), when the scan populated it.
    pub fn gap_equivalence_at(&self, index: usize) -> Option<&GapEquivalenceVerdict> {
        self.gap_equivalence.get(index)
    }

    fn is_gap_repairable(&self, gap: &Gap) -> bool {
        if !gap.is_fillable() {
            return false;
        }
        if self.limit_fill_to_mapped_region && self.gap_outside_reference_coverage(gap) {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap_with_b(start: Option<f64>, end: Option<f64>) -> Gap {
        Gap {
            video_a_start_secs: 10.0,
            video_a_end_secs: 20.0,
            video_b_start_secs: start,
            video_b_end_secs: end,
            b_has_energy: true,
        }
    }

    #[test]
    fn mapped_b_span_accepts_a_well_formed_span() {
        assert_eq!(
            gap_with_b(Some(3.0), Some(13.0)).mapped_b_span(),
            Some((3.0, 13.0))
        );
        assert_eq!(
            gap_with_b(Some(0.0), Some(1.0)).mapped_b_span(),
            Some((0.0, 1.0))
        );
    }

    #[test]
    fn mapped_b_span_rejects_half_mapped_rather_than_mixing_timelines() {
        // The F10 shape: a B start with no B end previously fell back to the *A* end.
        assert_eq!(gap_with_b(Some(3.0), None).mapped_b_span(), None);
        assert_eq!(gap_with_b(None, Some(13.0)).mapped_b_span(), None);
        assert_eq!(gap_with_b(None, None).mapped_b_span(), None);
    }

    #[test]
    fn mapped_b_span_rejects_negative_and_degenerate_spans() {
        // A negative mapped start is rejected outright, not clamped in one expression and
        // reported unclamped in the next.
        assert_eq!(gap_with_b(Some(-0.5), Some(9.5)).mapped_b_span(), None);
        assert_eq!(gap_with_b(Some(3.0), Some(3.0)).mapped_b_span(), None);
        assert_eq!(gap_with_b(Some(13.0), Some(3.0)).mapped_b_span(), None);
    }

    #[test]
    fn mapped_b_span_matches_b_range_fully_scanned_on_shape() {
        // Same predicate shape, different job: this one has no coverage limit. Any span
        // `mapped_b_span` accepts must be shape-acceptable to the coverage check too.
        for (start, end) in [(3.0, 13.0), (0.0, 1.0)] {
            assert!(gap_with_b(Some(start), Some(end)).mapped_b_span().is_some());
            assert!(crate::domain::cross_check::b_range_fully_scanned(
                start,
                end,
                Some(end)
            ));
        }
        for (start, end) in [(-0.5, 9.5), (3.0, 3.0), (13.0, 3.0)] {
            assert!(gap_with_b(Some(start), Some(end)).mapped_b_span().is_none());
            assert!(!crate::domain::cross_check::b_range_fully_scanned(
                start,
                end,
                Some(end.max(start))
            ));
        }
    }
}
