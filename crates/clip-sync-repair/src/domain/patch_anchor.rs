//! Empirical offset anchors from successful gap patches.

use crate::domain::align::{ClipRole, ScanAlignment};
use serde::Serialize;

use crate::domain::fill_offset::clip_anchor_points;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::GapPatchSkipReason;

/// One measured A→B offset at a patched gap midpoint on A's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PatchOffsetAnchor {
    pub a_secs: f64,
    pub offset_secs: f64,
    pub source_gap_index: usize,
    /// Interpolation weight from `min(pre, post)` seam scores at anchor time.
    pub weight: f64,
}

/// JSON-facing anchor row (exported on repair summary).
pub type PatchAnchorReport = PatchOffsetAnchor;

/// Policy for which pass-1 patches become anchors.
#[derive(Debug, Clone, Copy)]
pub struct PatchAnchorPolicy {
    pub min_correlation: f32,
    pub exclude_structure_trusted: bool,
    pub max_adjustment_frac: f64,
    pub border_search_secs: f64,
}

impl PatchAnchorPolicy {
    pub fn max_adjustment_secs(self) -> f64 {
        self.border_search_secs * self.max_adjustment_frac
    }
}

/// Sorted patch anchors built from a completed pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchAnchorTable {
    pub anchors: Vec<PatchOffsetAnchor>,
}

/// Input for one gap from a patch pass (application layer).
#[derive(Debug, Clone, Copy)]
pub struct PatchAnchorCandidate {
    pub gap_index: usize,
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub base_gap_offset_secs: f64,
    pub align_adjustment_secs: f64,
    pub pre_correlation: f64,
    pub post_correlation: f64,
    pub structure_trusted: bool,
    pub confidence: FillConfidence,
}

impl PatchAnchorTable {
    pub fn from_candidates(
        candidates: &[PatchAnchorCandidate],
        policy: &PatchAnchorPolicy,
    ) -> Self {
        let max_slide = policy.max_adjustment_secs();
        let mut anchors = Vec::new();
        for candidate in candidates {
            if !anchor_eligible(candidate, policy, max_slide) {
                continue;
            }
            let a_secs = (candidate.a_start_secs + candidate.a_end_secs) / 2.0;
            let weight = candidate
                .pre_correlation
                .min(candidate.post_correlation)
                .max(0.0);
            anchors.push(PatchOffsetAnchor {
                a_secs,
                offset_secs: candidate.base_gap_offset_secs + candidate.align_adjustment_secs,
                source_gap_index: candidate.gap_index,
                weight,
            });
        }
        anchors.sort_by(|a, b| {
            a.a_secs
                .partial_cmp(&b.a_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { anchors }
    }

    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn to_reports(&self) -> Vec<PatchAnchorReport> {
        self.anchors.to_vec()
    }
}

fn anchor_eligible(
    candidate: &PatchAnchorCandidate,
    policy: &PatchAnchorPolicy,
    max_slide: f64,
) -> bool {
    if candidate.confidence != FillConfidence::High {
        return false;
    }
    if policy.exclude_structure_trusted && candidate.structure_trusted {
        return false;
    }
    let min_score = candidate.pre_correlation.min(candidate.post_correlation);
    if min_score < f64::from(policy.min_correlation) {
        return false;
    }
    if candidate.align_adjustment_secs.abs() > max_slide {
        return false;
    }
    true
}

/// Interpolate offset at `gap_time_on_a_secs` from patch + clip anchors.
pub fn interpolate_anchored_offset_secs(
    alignment: &ScanAlignment,
    gap_time_on_a_secs: f64,
    patch_anchors: &PatchAnchorTable,
) -> Option<f64> {
    let points = weighted_anchor_points(alignment, patch_anchors);
    if points.is_empty() {
        return None;
    }
    Some(interpolate_piecewise_weighted(&points, gap_time_on_a_secs))
}

fn weighted_anchor_points(
    alignment: &ScanAlignment,
    patch_anchors: &PatchAnchorTable,
) -> Vec<(f64, f64, f64)> {
    let mut points = Vec::new();
    for (anchor, offset) in clip_anchor_points(alignment) {
        points.push((anchor, offset, 1.0));
    }
    for anchor in &patch_anchors.anchors {
        points.push((anchor.a_secs, anchor.offset_secs, anchor.weight.max(1e-9)));
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    dedupe_weighted_points(&mut points);
    points
}

fn dedupe_weighted_points(points: &mut Vec<(f64, f64, f64)>) {
    if points.is_empty() {
        return;
    }
    let mut merged: Vec<(f64, f64, f64)> = Vec::with_capacity(points.len());
    for (t, o, w) in points.drain(..) {
        if let Some(last) = merged.last_mut() {
            if (last.0 - t).abs() < 1e-9 {
                if w > last.2 {
                    *last = (t, o, w);
                }
                continue;
            }
        }
        merged.push((t, o, w));
    }
    *points = merged;
}

/// Soft prior for unified fit: penalize B candidates far from anchor-predicted start.
#[derive(Debug, Clone, Copy)]
pub struct AnchorSearchPrior {
    pub predicted_start_frame: usize,
    pub weight: f64,
    pub search_radius_frames: usize,
}

impl AnchorSearchPrior {
    pub fn penalty_at_start(self, start: usize) -> f64 {
        if self.weight <= 0.0 || self.search_radius_frames == 0 {
            return 0.0;
        }
        let dist = start.abs_diff(self.predicted_start_frame) as f64;
        self.weight * dist / self.search_radius_frames as f64
    }
}

fn interpolate_piecewise_weighted(points: &[(f64, f64, f64)], t: f64) -> f64 {
    if points.len() == 1 {
        return points[0].1;
    }
    if t <= points[0].0 {
        return points[0].1;
    }
    let last = points.len() - 1;
    if t >= points[last].0 {
        return points[last].1;
    }
    for window in points.windows(2) {
        let (t0, o0, w0) = window[0];
        let (t1, o1, w1) = window[1];
        if t >= t0 && t <= t1 {
            if (t1 - t0).abs() < f64::EPSILON {
                return o0;
            }
            let frac = (t - t0) / (t1 - t0);
            let w_left = w0 * (1.0 - frac);
            let w_right = w1 * frac;
            let denom = w_left + w_right;
            if denom < f64::EPSILON {
                return o0 + frac * (o1 - o0);
            }
            return (w_left * o0 + w_right * o1) / denom;
        }
    }
    points[last].1
}

/// Source of one point on the piecewise offset curve (verbose output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorPointSource {
    StartClip,
    EndClip,
    PatchGap(usize),
}

/// Verbose line: which anchors produced the pass-2 offset (`-v` stderr).
pub fn format_anchored_offset_verbose_line(
    gap_offset_secs: f64,
    alignment: &ScanAlignment,
    gap_time_on_a_secs: f64,
    patch_anchors: &PatchAnchorTable,
) -> String {
    let (left, right) = anchor_bracket_sources(alignment, gap_time_on_a_secs, patch_anchors);
    let source_desc = format_anchor_bracket(left, right);
    format!(
        "           offset anchor: {:+.3}s {source_desc}",
        gap_offset_secs
    )
}

/// Summary after pass 1 when building the anchor table (`-v` stderr).
pub fn format_patch_anchor_table_summary(table: &PatchAnchorTable) -> String {
    if table.anchors.is_empty() {
        return String::new();
    }
    let gaps: Vec<String> = table
        .anchors
        .iter()
        .map(|a| format!("gap #{}", a.source_gap_index + 1))
        .collect();
    format!(
        "  anchored: {} offset anchor(s) from {}",
        table.anchors.len(),
        gaps.join(", ")
    )
}

fn anchor_bracket_sources(
    alignment: &ScanAlignment,
    gap_time_on_a_secs: f64,
    patch_anchors: &PatchAnchorTable,
) -> (AnchorPointSource, AnchorPointSource) {
    let points = tagged_anchor_points(alignment, patch_anchors);
    if points.is_empty() {
        return (AnchorPointSource::StartClip, AnchorPointSource::StartClip);
    }
    if points.len() == 1 {
        return (points[0].1, points[0].1);
    }
    if gap_time_on_a_secs <= points[0].0 {
        return (points[0].1, points[0].1);
    }
    let last = points.len() - 1;
    if gap_time_on_a_secs >= points[last].0 {
        return (points[last].1, points[last].1);
    }
    for window in points.windows(2) {
        if gap_time_on_a_secs >= window[0].0 && gap_time_on_a_secs <= window[1].0 {
            return (window[0].1, window[1].1);
        }
    }
    (points[last].1, points[last].1)
}

fn tagged_anchor_points(
    alignment: &ScanAlignment,
    patch_anchors: &PatchAnchorTable,
) -> Vec<(f64, AnchorPointSource)> {
    let mut points = Vec::new();
    if let Some((anchor, _)) = clip_endpoint(alignment, ClipRole::Start) {
        points.push((anchor, AnchorPointSource::StartClip));
    }
    if let Some((anchor, _)) = clip_endpoint(alignment, ClipRole::End) {
        points.push((anchor, AnchorPointSource::EndClip));
    }
    for anchor in &patch_anchors.anchors {
        points.push((
            anchor.a_secs,
            AnchorPointSource::PatchGap(anchor.source_gap_index),
        ));
    }
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    points
}

fn clip_endpoint(alignment: &ScanAlignment, role: ClipRole) -> Option<(f64, f64)> {
    let clip = alignment
        .clips
        .iter()
        .find(|clip| clip.role == role && clip.aligned)?;
    let offset = clip.offset_secs?;
    let anchor = (clip.window_start_secs + clip.window_end_secs) / 2.0;
    Some((anchor, offset))
}

fn format_anchor_bracket(left: AnchorPointSource, right: AnchorPointSource) -> String {
    if left == right {
        format!("from {}", format_anchor_source(left))
    } else {
        format!(
            "between {} and {}",
            format_anchor_source(left),
            format_anchor_source(right)
        )
    }
}

fn format_anchor_source(source: AnchorPointSource) -> String {
    match source {
        AnchorPointSource::StartClip => "start clip".to_string(),
        AnchorPointSource::EndClip => "end clip".to_string(),
        AnchorPointSource::PatchGap(index) => format!("gap #{}", index + 1),
    }
}

/// Whether a skipped gap may be retried in pass 2 of `anchored_retry`.
pub fn is_retryable_patch_skip(reason: &GapPatchSkipReason) -> bool {
    matches!(
        reason,
        GapPatchSkipReason::BoundaryAlignmentFailed
            | GapPatchSkipReason::CorrelationBelowThreshold { .. }
            | GapPatchSkipReason::ProgramQuiet
    )
}

#[cfg(test)]
mod tests {
    use crate::domain::align::{test_empty_alignment, test_two_clip_alignment};

    use super::*;

    fn patch_only_alignment() -> ScanAlignment {
        test_empty_alignment(0.0)
    }

    fn two_clip_alignment(start_offset: f64, end_offset: f64) -> ScanAlignment {
        let mut alignment = test_two_clip_alignment(start_offset, end_offset);
        alignment.clips[0].window_end_secs = 50.0;
        alignment.clips[1].window_start_secs = 50.0;
        alignment.clips[1].window_end_secs = 100.0;
        alignment.recommended_offset_secs = Some(start_offset);
        alignment
    }

    fn candidate(gap_index: usize, a_mid: f64, base: f64, slide: f64) -> PatchAnchorCandidate {
        PatchAnchorCandidate {
            gap_index,
            a_start_secs: a_mid - 0.5,
            a_end_secs: a_mid + 0.5,
            base_gap_offset_secs: base,
            align_adjustment_secs: slide,
            pre_correlation: 0.9,
            post_correlation: 0.9,
            structure_trusted: false,
            confidence: FillConfidence::High,
        }
    }

    fn policy() -> PatchAnchorPolicy {
        PatchAnchorPolicy {
            min_correlation: 0.35,
            exclude_structure_trusted: true,
            max_adjustment_frac: 0.9,
            border_search_secs: 10.0,
        }
    }

    #[test]
    fn table_excludes_marginal_and_structure_trusted() {
        let mut marginal = candidate(0, 10.0, 0.0, 0.0);
        marginal.confidence = FillConfidence::Marginal;
        let mut trusted = candidate(1, 20.0, 0.0, 0.0);
        trusted.structure_trusted = true;
        let table = PatchAnchorTable::from_candidates(
            &[marginal, trusted, candidate(2, 30.0, 0.0, 0.5)],
            &policy(),
        );
        assert_eq!(table.anchors.len(), 1);
        assert!((table.anchors[0].offset_secs - 0.5).abs() < 1e-9);
    }

    #[test]
    fn weighted_anchor_prefers_higher_seam_score() {
        let alignment = patch_only_alignment();
        let mut weak = candidate(0, 10.0, 0.0, 0.0);
        weak.pre_correlation = 0.4;
        weak.post_correlation = 0.4;
        let mut strong = candidate(1, 30.0, 0.0, 2.0);
        strong.pre_correlation = 0.95;
        strong.post_correlation = 0.95;
        let table = PatchAnchorTable::from_candidates(&[weak, strong], &policy());
        let mid = interpolate_anchored_offset_secs(&alignment, 20.0, &table).unwrap();
        assert!(
            mid > 1.3,
            "weighted mid should lean toward strong anchor, got {mid}"
        );
    }

    #[test]
    fn interpolate_between_two_patch_anchors() {
        let alignment = patch_only_alignment();
        let table = PatchAnchorTable::from_candidates(
            &[candidate(0, 10.0, 0.0, 0.0), candidate(1, 30.0, 0.0, 2.0)],
            &policy(),
        );
        let mid = interpolate_anchored_offset_secs(&alignment, 20.0, &table).unwrap();
        assert!((mid - 1.0).abs() < 0.01);
    }

    #[test]
    fn extrapolation_clamps_to_endpoints() {
        let alignment = patch_only_alignment();
        let table = PatchAnchorTable::from_candidates(&[candidate(0, 50.0, 0.0, 1.0)], &policy());
        let before = interpolate_anchored_offset_secs(&alignment, 5.0, &table).unwrap();
        let after = interpolate_anchored_offset_secs(&alignment, 95.0, &table).unwrap();
        assert!((before - 1.0).abs() < 1e-9);
        assert!((after - 1.0).abs() < 1e-9);
    }

    #[test]
    fn merges_clip_endpoints_with_patch_anchors() {
        let alignment = two_clip_alignment(0.0, 2.0);
        let table = PatchAnchorTable::from_candidates(&[candidate(0, 50.0, 0.0, 1.0)], &policy());
        let late = interpolate_anchored_offset_secs(&alignment, 72.0, &table).unwrap();
        assert!(late > 1.0 && late < 2.0, "late={late}");
    }

    #[test]
    fn verbose_line_single_patch_anchor() {
        let alignment = patch_only_alignment();
        let table = PatchAnchorTable::from_candidates(&[candidate(2, 30.0, 0.0, 0.35)], &policy());
        let line = format_anchored_offset_verbose_line(0.35, &alignment, 30.0, &table);
        assert!(line.contains("offset anchor: +0.350s"));
        assert!(line.contains("from gap #3"));
    }

    #[test]
    fn verbose_line_interpolates_between_gaps() {
        let alignment = patch_only_alignment();
        let table = PatchAnchorTable::from_candidates(
            &[candidate(0, 10.0, 0.0, 0.0), candidate(1, 30.0, 0.0, 2.0)],
            &policy(),
        );
        let line = format_anchored_offset_verbose_line(1.0, &alignment, 20.0, &table);
        assert!(line.contains("between gap #1 and gap #2"));
    }

    #[test]
    fn table_summary_lists_source_gaps() {
        let table = PatchAnchorTable::from_candidates(
            &[candidate(0, 10.0, 0.0, 0.0), candidate(2, 30.0, 0.0, 0.5)],
            &policy(),
        );
        let summary = format_patch_anchor_table_summary(&table);
        assert!(summary.contains("2 offset anchor(s)"));
        assert!(summary.contains("gap #1"));
        assert!(summary.contains("gap #3"));
    }
}
