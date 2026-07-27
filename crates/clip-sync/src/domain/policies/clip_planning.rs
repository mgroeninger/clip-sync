//! Symmetric multi-clip window planning and Auto query-mode gating.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::domain::alignment::{clip_with_label, AlignmentResult};
use crate::domain::clip_plan::ClipPlan;
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::error::DomainError;
use crate::domain::media_extent::MediaExtent;
use crate::domain::query_localization::AlignmentModeUsed;

/// How symmetric multi-clip planning anchors the end window (and interior when `num_clips > 2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndClipAnchor {
    /// Each file's last `clip_length` before its own effective timeline end.
    FileTail,
    /// Shorter effective duration defines shared absolute times on both timelines.
    #[default]
    #[serde(alias = "anchored")]
    SharedTimeline,
}

/// Interior window overlapping start/end by more than this is omitted (paired planning).
pub const INTERIOR_OVERLAP_TOLERANCE: Duration = Duration::from_secs(1);

/// Optional bounds when placing multi-clip windows near the file tail.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClipPlanningOptions {
    /// Inset before [`MediaExtent::effective`] for the end clip window.
    pub end_tail_inset: Duration,
    /// End (and interior when `num_clips > 2`) anchoring for paired symmetric planning.
    pub end_clip_anchor: EndClipAnchor,
}

pub fn effective_timeline_end(extent: &MediaExtent, end_tail_inset: Duration) -> Duration {
    extent
        .effective()
        .saturating_sub(end_tail_inset)
        .max(Duration::from_secs(1))
        .min(extent.declared)
}

pub fn clip_windows_with_options(
    extent: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<Vec<ClipWindow>, DomainError> {
    if extent.declared.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

    let duration = extent.declared;
    let clip_length = plan.clip_length;
    let effective_num_clips = if duration < clip_length {
        1
    } else {
        plan.num_clips
    };

    if effective_num_clips == 1 {
        let end = duration.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(Duration::ZERO, end, ClipLabel::Start)]);
    }

    let timeline_end = effective_timeline_end(extent, options.end_tail_inset);
    if timeline_end < clip_length {
        let end = timeline_end.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        return Ok(vec![ClipWindow::new(Duration::ZERO, end, ClipLabel::Start)]);
    }

    let n = effective_num_clips;
    let mut windows = Vec::with_capacity(n as usize);

    windows.push(ClipWindow::new(
        Duration::ZERO,
        clip_length,
        ClipLabel::Start,
    ));

    if n > 2 {
        windows.extend(interior_windows_along_timeline(
            timeline_end.as_secs_f64(),
            clip_length,
            n,
        ));
    }

    let end_start = timeline_end.saturating_sub(clip_length);
    windows.push(ClipWindow::new(end_start, timeline_end, ClipLabel::End));

    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }

    Ok(windows)
}

/// Interior windows for segment indices `1..n-1` along `timeline_secs`.
pub fn interior_windows_along_timeline(
    timeline_secs: f64,
    clip_length: Duration,
    n: u32,
) -> Vec<ClipWindow> {
    if n <= 2 {
        return Vec::new();
    }
    let clip_secs = clip_length.as_secs_f64();
    let mut windows = Vec::with_capacity((n.saturating_sub(2)) as usize);
    for i in 1..(n - 1) {
        let seg_start_secs = timeline_secs * f64::from(i) / f64::from(n);
        let seg_end_secs = timeline_secs * f64::from(i + 1) / f64::from(n);
        let center_secs = (seg_start_secs + seg_end_secs) / 2.0;
        let half = clip_secs / 2.0;
        let start_secs = (center_secs - half).max(0.0);
        let end_secs = (start_secs + clip_secs).min(timeline_secs);
        windows.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(end_secs),
            ClipLabel::Interior,
        ));
    }
    windows
}

/// Whether `interior` shares more than `tolerance` with `start` or `end`.
pub fn interior_overlaps_fixed_clip(
    interior: &ClipWindow,
    start: &ClipWindow,
    end: &ClipWindow,
    tolerance: Duration,
) -> bool {
    let tol = tolerance.as_secs_f64();
    window_overlap_secs(interior, start) > tol || window_overlap_secs(interior, end) > tol
}

fn window_overlap_secs(a: &ClipWindow, b: &ClipWindow) -> f64 {
    let start = a.start.max(b.start);
    let end = a.end.min(b.end);
    if end <= start {
        0.0
    } else {
        (end - start).as_secs_f64()
    }
}

/// Paired symmetric clip windows for two files (see anchored-end extraction plan).
pub fn clip_windows_paired(
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
) -> Result<(Vec<ClipWindow>, Vec<ClipWindow>), DomainError> {
    if extent_a.declared.is_zero() || extent_b.declared.is_zero() {
        return Err(DomainError::InvalidDuration);
    }

    if plan.num_clips < 2 {
        let windows_a = clip_windows_with_options(extent_a, plan, options)?;
        let windows_b = clip_windows_with_options(extent_b, plan, options)?;
        return Ok((windows_a, windows_b));
    }

    let clip_length = plan.clip_length;
    let timeline_end_a = effective_timeline_end(extent_a, options.end_tail_inset);
    let timeline_end_b = effective_timeline_end(extent_b, options.end_tail_inset);
    let t_anchor = timeline_end_a.min(timeline_end_b);

    if t_anchor < clip_length || extent_a.declared < clip_length || extent_b.declared < clip_length
    {
        let end = t_anchor.min(clip_length);
        if end.is_zero() {
            return Err(DomainError::EmptyClip);
        }
        let window = ClipWindow::new(Duration::ZERO, end, ClipLabel::Start);
        return Ok((vec![window], vec![window]));
    }

    let n = plan.num_clips;
    let start = ClipWindow::new(Duration::ZERO, clip_length, ClipLabel::Start);
    let end_a = end_window_for_file(
        timeline_end_a,
        t_anchor,
        clip_length,
        options.end_clip_anchor,
    );
    let end_b = end_window_for_file(
        timeline_end_b,
        t_anchor,
        clip_length,
        options.end_clip_anchor,
    );

    let (interiors_a, interiors_b) = match options.end_clip_anchor {
        EndClipAnchor::FileTail => {
            let interiors_a =
                interior_windows_along_timeline(timeline_end_a.as_secs_f64(), clip_length, n);
            let interiors_b =
                interior_windows_along_timeline(timeline_end_b.as_secs_f64(), clip_length, n);
            filter_overlapping_interiors_paired(
                interiors_a,
                interiors_b,
                &start,
                &end_a,
                &start,
                &end_b,
            )
        }
        EndClipAnchor::SharedTimeline => {
            let interiors = interior_windows_along_timeline(t_anchor.as_secs_f64(), clip_length, n)
                .into_iter()
                .filter(|interior| {
                    !interior_overlaps_fixed_clip(
                        interior,
                        &start,
                        &end_a,
                        INTERIOR_OVERLAP_TOLERANCE,
                    )
                })
                .collect::<Vec<_>>();
            (interiors.clone(), interiors)
        }
    };

    let windows_a = assemble_labeled_windows(start, interiors_a, end_a)?;
    let windows_b = assemble_labeled_windows(start, interiors_b, end_b)?;
    debug_assert_eq!(windows_a.len(), windows_b.len());
    Ok((windows_a, windows_b))
}

/// Attaches symmetric clip-planning metadata for reporting (end anchor + per-clip B windows).
pub fn attach_symmetric_planning_report_metadata(
    result: &mut AlignmentResult,
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    plan: &ClipPlan,
    options: ClipPlanningOptions,
    num_clips_configured: u32,
) {
    if result.query_localization.is_some()
        || result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference)
    {
        return;
    }
    if num_clips_configured < 2 || clip_with_label(&result.clips, ClipLabel::End).is_none() {
        return;
    }

    result.end_clip_anchor = Some(options.end_clip_anchor);

    let Ok((windows_a, windows_b)) = clip_windows_paired(extent_a, extent_b, plan, options) else {
        return;
    };
    if windows_a.len() != result.clips.len() {
        return;
    }

    for (clip, (window_a, window_b)) in result
        .clips
        .iter_mut()
        .zip(windows_a.iter().zip(windows_b.iter()))
    {
        let a_start = window_a.start.as_secs_f64();
        let a_end = window_a.end.as_secs_f64();
        let b_start = window_b.start.as_secs_f64();
        let b_end = window_b.end.as_secs_f64();
        if (b_start - a_start).abs() > 1e-9 || (b_end - a_end).abs() > 1e-9 {
            clip.video_b_window_start_secs = Some(b_start);
            clip.video_b_window_end_secs = Some(b_end);
        } else {
            clip.video_b_window_start_secs = None;
            clip.video_b_window_end_secs = None;
        }
    }
}

fn end_window_for_file(
    timeline_end: Duration,
    t_anchor: Duration,
    clip_length: Duration,
    anchor: EndClipAnchor,
) -> ClipWindow {
    let end = match anchor {
        EndClipAnchor::FileTail => timeline_end,
        EndClipAnchor::SharedTimeline => t_anchor.min(timeline_end),
    };
    let start = end.saturating_sub(clip_length);
    ClipWindow::new(start, end, ClipLabel::End)
}

fn filter_overlapping_interiors_paired(
    interiors_a: Vec<ClipWindow>,
    interiors_b: Vec<ClipWindow>,
    start_a: &ClipWindow,
    end_a: &ClipWindow,
    start_b: &ClipWindow,
    end_b: &ClipWindow,
) -> (Vec<ClipWindow>, Vec<ClipWindow>) {
    debug_assert_eq!(interiors_a.len(), interiors_b.len());
    let mut out_a = Vec::with_capacity(interiors_a.len());
    let mut out_b = Vec::with_capacity(interiors_b.len());
    for (ia, ib) in interiors_a.into_iter().zip(interiors_b) {
        if interior_overlaps_fixed_clip(&ia, start_a, end_a, INTERIOR_OVERLAP_TOLERANCE)
            || interior_overlaps_fixed_clip(&ib, start_b, end_b, INTERIOR_OVERLAP_TOLERANCE)
        {
            continue;
        }
        out_a.push(ia);
        out_b.push(ib);
    }
    (out_a, out_b)
}

fn assemble_labeled_windows(
    start: ClipWindow,
    interiors: Vec<ClipWindow>,
    end: ClipWindow,
) -> Result<Vec<ClipWindow>, DomainError> {
    let mut windows = Vec::with_capacity(2 + interiors.len());
    windows.push(start);
    windows.extend(interiors);
    windows.push(end);
    for window in &windows {
        if window.duration().is_zero() {
            return Err(DomainError::EmptyClip);
        }
    }
    Ok(windows)
}

/// Two-tier query-mode decision for `Auto`: shorter file is treated as a query when it is
/// much shorter than the reference (Tier 1, durations only) or — for near-equal lengths —
/// when symmetric clip planning yields different window counts (Tier 2).
///
/// `query_min_duration_ratio` must be in `(0, 1]`. `windows_a` / `windows_b` are the symmetric
/// clip-window counts; callers may compute them lazily (only Tier 2 needs them).
pub fn should_use_query_mode(
    extent_a: &MediaExtent,
    extent_b: &MediaExtent,
    windows_a: usize,
    windows_b: usize,
    query_min_duration_ratio: f64,
) -> bool {
    let dur_a = extent_a.effective().as_secs_f64();
    let dur_b = extent_b.effective().as_secs_f64();
    let (short, long) = if dur_a <= dur_b {
        (dur_a, dur_b)
    } else {
        (dur_b, dur_a)
    };
    if long <= 0.0 {
        return false;
    }
    // Tier 1: duration ratio.
    if short / long < query_min_duration_ratio {
        return true;
    }
    // Tier 2: clip-window count mismatch (lengths are similar).
    windows_a != windows_b
}

pub(crate) fn secs_to_duration(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::clip_plan::ClipPlan;
    use crate::domain::clip_window::{ClipLabel, ClipWindow};
    use crate::domain::error::DomainError;
    use crate::domain::media_extent::MediaExtent;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    fn paired_options(anchor: EndClipAnchor) -> ClipPlanningOptions {
        ClipPlanningOptions {
            end_clip_anchor: anchor,
            ..Default::default()
        }
    }

    fn assert_windows_match_single(
        extent: &MediaExtent,
        plan: &ClipPlan,
        options: ClipPlanningOptions,
        paired: &[ClipWindow],
    ) {
        let single = clip_windows_with_options(extent, plan, options).unwrap();
        assert_eq!(single.len(), paired.len());
        for (expected, actual) in single.iter().zip(paired) {
            assert_eq!(expected.start, actual.start);
            assert_eq!(expected.end, actual.end);
            assert_eq!(expected.label, actual.label);
        }
    }

    #[test]
    fn clip_windows_short_media_single_start_clip() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(12)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(12));
        assert_eq!(windows[0].label, ClipLabel::Start);
    }

    #[test]
    fn clip_windows_two_clips_start_and_end() {
        let plan = ClipPlan::new(mins(15), 2);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(45)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(15));
        assert_eq!(windows[1].start, mins(30));
        assert_eq!(windows[1].end, mins(45));
    }

    #[test]
    fn clip_windows_num_clips_one_on_long_media() {
        let plan = ClipPlan::new(mins(15), 1);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end, mins(15));
    }

    #[test]
    fn clip_windows_three_clips_with_interior() {
        let plan = ClipPlan::new(mins(10), 3);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();

        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].label, ClipLabel::Start);
        assert_eq!(windows[1].label, ClipLabel::Interior);
        assert_eq!(windows[2].label, ClipLabel::End);
        assert_eq!(windows[0].start, Duration::ZERO);
        assert_eq!(windows[0].end, mins(10));
        assert_eq!(windows[1].start, mins(25));
        assert_eq!(windows[1].end, mins(35));
        assert_eq!(windows[2].start, mins(50));
        assert_eq!(windows[2].end, mins(60));
    }

    #[test]
    fn clip_windows_clamps_end_to_decodable_extent_with_inset() {
        let plan = ClipPlan::new(mins(15), 2);
        let container = Duration::from_secs_f64(6180.033);
        let extent = Duration::from_secs_f64(6176.0);
        let windows = clip_windows_with_options(
            &MediaExtent::new(container, Some(extent)),
            &plan,
            ClipPlanningOptions {
                end_tail_inset: Duration::from_secs(1),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(windows.len(), 2);
        let end = &windows[1];
        assert_eq!(end.label, ClipLabel::End);
        assert!((end.end.as_secs_f64() - 6175.0).abs() < 0.01);
        assert!((end.start.as_secs_f64() - (6175.0 - 900.0)).abs() < 0.01);
    }

    #[test]
    fn clip_windows_rejects_zero_duration() {
        let plan = ClipPlan::new(mins(15), 2);
        assert_eq!(
            clip_windows_with_options(
                &MediaExtent::from_declared(Duration::ZERO),
                &plan,
                ClipPlanningOptions::default(),
            ),
            Err(DomainError::InvalidDuration)
        );
    }

    #[test]
    fn clip_windows_paired_equal_45_min_matches_single_file_planner() {
        let plan = ClipPlan::new(mins(15), 2);
        let extent = MediaExtent::from_declared(mins(45));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&extent, &extent, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_windows_match_single(&extent, &plan, options, &a);
    }

    #[test]
    fn clip_windows_paired_equal_45_min_three_clips_matches_single_file() {
        let plan = ClipPlan::new(mins(10), 3);
        let extent = MediaExtent::from_declared(mins(60));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&extent, &extent, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_windows_match_single(&extent, &plan, options, &a);
    }

    #[test]
    fn clip_windows_paired_40_300_shared_timeline_end_on_both() {
        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a.len(), b.len());
        assert_eq!(a[1].start, mins(25));
        assert_eq!(a[1].end, mins(40));
        assert_eq!(b[1].start, mins(25));
        assert_eq!(b[1].end, mins(40));
        assert_ne!(b[1].end, mins(300));
    }

    #[test]
    fn clip_windows_paired_40_300_file_tail_b_end_at_file_tail() {
        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::FileTail);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a[1].start, mins(25));
        assert_eq!(a[1].end, mins(40));
        assert_eq!(b[1].start, mins(285));
        assert_eq!(b[1].end, mins(300));
    }

    #[test]
    fn clip_windows_paired_40_300_three_clips_shared_interior_and_end() {
        let plan = ClipPlan::new(mins(10), 3);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
        assert_eq!(a[1].label, ClipLabel::Interior);
        assert_eq!(a[1].start, mins(15));
        assert_eq!(a[1].end, mins(25));
        assert_eq!(a[2].start, mins(30));
        assert_eq!(a[2].end, mins(40));
    }

    #[test]
    fn clip_windows_paired_collapses_12_20_min_to_single_window() {
        let plan = ClipPlan::new(mins(15), 2);
        let a_extent = MediaExtent::from_declared(mins(12));
        let b_extent = MediaExtent::from_declared(mins(20));
        let options = paired_options(EndClipAnchor::SharedTimeline);
        let (a, b) = clip_windows_paired(&a_extent, &b_extent, &plan, options).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].end, mins(12));
        assert_eq!(b[0].end, mins(12));
    }

    #[test]
    fn clip_windows_paired_end_tail_inset_before_anchor_min() {
        let plan = ClipPlan::new(mins(15), 2);
        let container = Duration::from_secs(300 * 60);
        let short_decodable = Duration::from_secs(40 * 60);
        let long_decodable = Duration::from_secs(300 * 60);
        let short = MediaExtent::new(container, Some(short_decodable));
        let long = MediaExtent::new(container, Some(long_decodable));
        let options = ClipPlanningOptions {
            end_tail_inset: Duration::from_secs(60),
            end_clip_anchor: EndClipAnchor::SharedTimeline,
        };
        let (a, b) = clip_windows_paired(&short, &long, &plan, options).unwrap();
        let t_anchor = short_decodable.saturating_sub(Duration::from_secs(60));
        assert_eq!(a[1].end, t_anchor);
        assert_eq!(b[1].end, t_anchor);
    }

    #[test]
    fn interior_windows_along_timeline_matches_three_clip_fixture() {
        let from_helper = interior_windows_along_timeline(3600.0, mins(10), 3);
        let plan = ClipPlan::new(mins(10), 3);
        let windows = clip_windows_with_options(
            &MediaExtent::from_declared(mins(60)),
            &plan,
            ClipPlanningOptions::default(),
        )
        .unwrap();
        assert_eq!(from_helper.len(), 1);
        assert_eq!(from_helper[0], windows[1]);
    }

    #[test]
    fn attach_symmetric_planning_report_metadata_sets_anchor_and_b_windows() {
        use crate::domain::alignment::{
            build_alignment_result, ClipMatchEstimate, ClipPairReportInput,
        };

        let plan = ClipPlan::new(mins(15), 2);
        let short = MediaExtent::from_declared(mins(40));
        let long = MediaExtent::from_declared(mins(300));
        let windows =
            clip_windows_with_options(&short, &plan, ClipPlanningOptions::default()).unwrap();
        let estimates = windows
            .iter()
            .map(|_| ClipMatchEstimate {
                offset_secs: 12.0,
                confidence: 0.9,
            })
            .collect::<Vec<_>>();
        let mut result = build_alignment_result(
            ClipPairReportInput {
                windows: &windows,
                estimates: &estimates,
                decode_skips_a: &[],
                decode_skips_b: &[],
                duration_a: Some(short.declared),
                duration_b: Some(long.declared),
            },
            crate::domain::alignment::AlignmentMergePolicy {
                min_match_score: 0.5,
                prefer_start_clip: true,
                require_consistent_offsets: false,
            },
        );
        attach_symmetric_planning_report_metadata(
            &mut result,
            &short,
            &long,
            &plan,
            paired_options(EndClipAnchor::FileTail),
            2,
        );
        assert_eq!(result.end_clip_anchor, Some(EndClipAnchor::FileTail));
        let end = result
            .clips
            .iter()
            .find(|clip| clip.label == ClipLabel::End)
            .expect("end clip");
        assert_eq!(end.video_b_window_start_secs, Some(mins(285).as_secs_f64()));
        assert_eq!(end.video_b_window_end_secs, Some(mins(300).as_secs_f64()));
    }
}
