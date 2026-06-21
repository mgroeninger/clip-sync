//! Per-gap B timeline offset from alignment clip pair (recommended vs interpolated drift).

use clip_sync::{AlignmentReport, ClipLabelReport};

use crate::domain::patch_anchor::{interpolate_anchored_offset_secs, PatchAnchorTable};

/// How patch maps each gap on A to B's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillOffsetMode {
    /// Use `recommended_offset_secs` for every gap (scan + patch).
    #[default]
    Recommended,
    /// Linearly interpolate between start-clip and end-clip offsets by position on A.
    Interpolated,
    /// Interpolate from patch anchors plus clip endpoints (single-pass; not wired in patch yet).
    Anchored,
    /// Pass 1: clip-based offset; pass 2: retry seam failures using anchors from pass-1 successes.
    AnchoredRetry,
}

/// Which pass of [`FillOffsetMode::AnchoredRetry`] is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnchoredRetryPass {
    #[default]
    First,
    Second,
}

/// Minimum |end − start| clip offset drift before interpolation differs from a single offset.
const MIN_DRIFT_FOR_INTERPOLATION_SECS: f64 = 0.05;

/// Offset (seconds) to add to A's timeline to reach B: `b_secs = a_secs + offset`.
pub fn fill_offset_secs(
    alignment: &AlignmentReport,
    mode: FillOffsetMode,
    gap_time_on_a_secs: f64,
) -> Option<f64> {
    resolve_gap_offset_secs(alignment, mode, gap_time_on_a_secs, None, AnchoredRetryPass::First)
}

/// Resolve per-gap B offset, optionally using patch anchors from a prior pass.
pub fn resolve_gap_offset_secs(
    alignment: &AlignmentReport,
    mode: FillOffsetMode,
    gap_time_on_a_secs: f64,
    patch_anchors: Option<&PatchAnchorTable>,
    anchored_retry_pass: AnchoredRetryPass,
) -> Option<f64> {
    match mode {
        FillOffsetMode::Recommended => alignment.recommended_offset_secs,
        FillOffsetMode::Interpolated => clip_only_offset_secs(alignment, gap_time_on_a_secs),
        FillOffsetMode::Anchored => anchored_offset_secs(alignment, gap_time_on_a_secs, patch_anchors),
        FillOffsetMode::AnchoredRetry => match anchored_retry_pass {
            AnchoredRetryPass::First => clip_only_offset_secs(alignment, gap_time_on_a_secs),
            AnchoredRetryPass::Second => {
                anchored_offset_secs(alignment, gap_time_on_a_secs, patch_anchors)
            }
        },
    }
}

fn clip_only_offset_secs(
    alignment: &AlignmentReport,
    gap_time_on_a_secs: f64,
) -> Option<f64> {
    interpolated_offset_secs(alignment, gap_time_on_a_secs)
        .or(alignment.recommended_offset_secs)
}

fn anchored_offset_secs(
    alignment: &AlignmentReport,
    gap_time_on_a_secs: f64,
    patch_anchors: Option<&PatchAnchorTable>,
) -> Option<f64> {
    if let Some(table) = patch_anchors.filter(|t| !t.is_empty()) {
        if let Some(offset) = interpolate_anchored_offset_secs(alignment, gap_time_on_a_secs, table)
        {
            return Some(offset);
        }
    }
    clip_only_offset_secs(alignment, gap_time_on_a_secs)
}

/// Clip start/end anchors for piecewise offset curves (`(a_mid_secs, offset_secs)`).
pub(crate) fn clip_anchor_points(alignment: &AlignmentReport) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    if let Some((offset, anchor)) = clip_offset_and_anchor(alignment, ClipLabelReport::Start) {
        points.push((anchor, offset));
    }
    if let Some((offset, anchor)) = clip_offset_and_anchor(alignment, ClipLabelReport::End) {
        points.push((anchor, offset));
    }
    points
}

pub(crate) fn interpolated_offset_secs(
    alignment: &AlignmentReport,
    gap_time_on_a_secs: f64,
) -> Option<f64> {
    let (start_offset, start_anchor) = clip_offset_and_anchor(alignment, ClipLabelReport::Start)?;
    let (end_offset, end_anchor) = clip_offset_and_anchor(alignment, ClipLabelReport::End)?;

    let drift = end_offset - start_offset;
    if drift.abs() < MIN_DRIFT_FOR_INTERPOLATION_SECS {
        return Some(start_offset);
    }

    let span = end_anchor - start_anchor;
    if span.abs() < f64::EPSILON {
        return Some(start_offset);
    }

    let t = gap_time_on_a_secs.clamp(start_anchor.min(end_anchor), start_anchor.max(end_anchor));
    let fraction = (t - start_anchor) / span;
    Some(start_offset + drift * fraction)
}

fn clip_offset_and_anchor(
    alignment: &AlignmentReport,
    label: ClipLabelReport,
) -> Option<(f64, f64)> {
    let clip = alignment
        .clips
        .iter()
        .find(|clip| clip.label == label && clip.aligned)?;
    let offset = clip.offset_secs?;
    let anchor = (clip.window_start_secs + clip.window_end_secs) / 2.0;
    Some((offset, anchor))
}

#[cfg(test)]
mod tests {
    use clip_sync::{AlignmentReport, AlignmentResult, ClipLabel, ClipLabelReport, ClipMatch};

    use crate::domain::patch_anchor::{PatchAnchorCandidate, PatchAnchorPolicy, PatchAnchorTable};

    use super::*;

    fn two_clip_alignment(start_offset: f64, end_offset: f64) -> AlignmentReport {
        AlignmentReport::from(&AlignmentResult {
            clips: vec![
                ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: true,
                    offset_secs: Some(start_offset),
                    confidence: 0.95,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                },
                ClipMatch {
                    label: ClipLabel::End,
                    window_start_secs: 6647.0,
                    window_end_secs: 7547.0,
                    aligned: true,
                    offset_secs: Some(end_offset),
                    confidence: 0.95,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                },
            ],
            start_aligned: true,
            end_aligned: Some(true),
            recommended_offset_secs: Some((start_offset + end_offset) / 2.0),
            offsets_consistent: false,
            offset_drift_secs: Some(end_offset - start_offset),
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
            offset_ambiguous_mod_secs: None,
            alignment_mode_used: None,
            query_localization: None,
            end_clip_anchor: None,
        })
    }

    #[test]
    fn recommended_mode_uses_headline_offset() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let offset = fill_offset_secs(
            &alignment,
            FillOffsetMode::Recommended,
            1610.0,
        )
        .unwrap();
        assert!((offset - (-7.0)).abs() < 0.01);
    }

    #[test]
    fn interpolated_offset_at_start_anchor_matches_start_clip() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let start_anchor = 450.0;
        let offset = fill_offset_secs(
            &alignment,
            FillOffsetMode::Interpolated,
            start_anchor,
        )
        .unwrap();
        assert!((offset - (-7.326)).abs() < 0.001);
    }

    #[test]
    fn interpolated_offset_at_mid_timeline() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let start_anchor = 450.0;
        let end_anchor = 7097.0;
        let mid = (start_anchor + end_anchor) / 2.0;
        let offset = fill_offset_secs(&alignment, FillOffsetMode::Interpolated, mid).unwrap();
        let expected = -7.326 + 0.652 * 0.5;
        assert!((offset - expected).abs() < 0.01);
    }

    #[test]
    fn interpolated_falls_back_when_only_start_clip() {
        let mut alignment = two_clip_alignment(-3.0, -2.0);
        alignment.clips.retain(|c| c.label == ClipLabelReport::Start);
        alignment.recommended_offset_secs = Some(-3.0);
        let offset = fill_offset_secs(&alignment, FillOffsetMode::Interpolated, 100.0).unwrap();
        assert!((offset - (-3.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn anchored_retry_first_pass_matches_clip_only() {
        let alignment = two_clip_alignment(0.0, 1.0);
        let clip = resolve_gap_offset_secs(
            &alignment,
            FillOffsetMode::AnchoredRetry,
            75.0,
            None,
            AnchoredRetryPass::First,
        )
        .unwrap();
        let interpolated = fill_offset_secs(&alignment, FillOffsetMode::Interpolated, 75.0).unwrap();
        assert!((clip - interpolated).abs() < 1e-9);
    }

    #[test]
    fn anchored_uses_patch_table() {
        let alignment = AlignmentReport::from(&AlignmentResult {
            clips: vec![],
            start_aligned: false,
            end_aligned: None,
            recommended_offset_secs: Some(0.0),
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
            offset_ambiguous_mod_secs: None,
            alignment_mode_used: None,
            query_localization: None,
            end_clip_anchor: None,
        });
        let policy = PatchAnchorPolicy {
            min_correlation: 0.35,
            exclude_structure_trusted: true,
            max_adjustment_frac: 0.9,
            border_search_secs: 30.0,
        };
        let table = PatchAnchorTable::from_candidates(
            &[PatchAnchorCandidate {
                gap_index: 0,
                a_start_secs: 9.5,
                a_end_secs: 10.5,
                base_gap_offset_secs: 0.0,
                align_adjustment_secs: 1.5,
                pre_correlation: 0.9,
                post_correlation: 0.9,
                structure_trusted: false,
                confidence: crate::domain::FillConfidence::High,
            }],
            &policy,
        );
        let offset = resolve_gap_offset_secs(
            &alignment,
            FillOffsetMode::Anchored,
            50.0,
            Some(&table),
            AnchoredRetryPass::First,
        )
        .unwrap();
        assert!((offset - 1.5).abs() < 1e-9);
    }
}
