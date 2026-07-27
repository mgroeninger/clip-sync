//! Hold-out window placement for offset verification and high-rate refinement.

use std::time::Duration;

use crate::domain::alignment::{AlignmentResult, TimelineOverlap};
use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::media_extent::MediaExtent;
use crate::domain::query_localization::AlignmentModeUsed;

use super::clip_planning::secs_to_duration;

/// Pick a hold-out window on the shorter file's timeline, avoiding discovery windows when possible.
pub fn pick_holdout_window(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
) -> Option<ClipWindow> {
    if duration < segment_length {
        return None;
    }

    let segment_secs = segment_length.as_secs_f64();
    let duration_secs = duration.as_secs_f64();

    if discovery_windows.len() <= 1 {
        let start_secs = duration_secs / 3.0;
        let end_secs = (start_secs + segment_secs).min(duration_secs);
        if end_secs - start_secs < segment_secs {
            return None;
        }
        return Some(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(end_secs),
            ClipLabel::Interior,
        ));
    }

    let gap_start = discovery_windows.first()?.end.as_secs_f64();
    let gap_end = discovery_windows.last()?.start.as_secs_f64();
    let start_secs = if gap_end - gap_start >= segment_secs {
        gap_start + (gap_end - gap_start - segment_secs) / 2.0
    } else {
        (duration_secs - segment_secs) / 2.0
    };
    let end_secs = start_secs + segment_secs;
    if end_secs > duration_secs {
        return None;
    }

    Some(ClipWindow::new(
        secs_to_duration(start_secs),
        secs_to_duration(end_secs),
        ClipLabel::Interior,
    ))
}

/// Hold-out placement candidates, most preferred first. Includes fallbacks for inflated duration metadata.
pub fn holdout_window_candidates(
    duration: Duration,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
    offset_secs: f64,
) -> Vec<ClipWindow> {
    if duration < segment_length {
        return Vec::new();
    }

    let duration_secs = duration.as_secs_f64();
    let segment_secs = segment_length.as_secs_f64();
    let mut candidates = Vec::new();

    let mut push_unique = |window: ClipWindow| {
        if window.end <= window.start {
            return;
        }
        if candidates.iter().any(|existing: &ClipWindow| {
            (existing.start.as_secs_f64() - window.start.as_secs_f64()).abs() < 0.001
        }) {
            return;
        }
        candidates.push(window);
    };

    // Overlap-safe near-start windows first (avoids broken mid-file seeks on some MKV tracks).
    let min_a_start = (-offset_secs).max(0.0);
    if min_a_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(min_a_start),
            secs_to_duration(min_a_start + segment_secs),
            ClipLabel::Interior,
        ));
    }
    let overlap_interior = min_a_start + 30.0;
    if overlap_interior + segment_secs <= duration_secs
        && overlap_interior > min_a_start + segment_secs
    {
        push_unique(ClipWindow::new(
            secs_to_duration(overlap_interior),
            secs_to_duration(overlap_interior + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(window) = pick_holdout_window(duration, discovery_windows, segment_length) {
        push_unique(window);
    }

    let early_start = duration_secs / 6.0;
    if early_start + segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            secs_to_duration(early_start),
            secs_to_duration(early_start + segment_secs),
            ClipLabel::Interior,
        ));
    }

    if let Some(first) = discovery_windows.first() {
        let after_discovery = first.end.as_secs_f64();
        if after_discovery + segment_secs <= duration_secs {
            push_unique(ClipWindow::new(
                secs_to_duration(after_discovery),
                secs_to_duration(after_discovery + segment_secs),
                ClipLabel::Interior,
            ));
        }
    }

    if segment_secs <= duration_secs {
        push_unique(ClipWindow::new(
            Duration::ZERO,
            secs_to_duration(segment_secs),
            ClipLabel::Interior,
        ));
    }

    candidates
}

/// Calendar-parallel hold-out windows (same `[T, T+L)` on A and B) for periodic offset recheck.
/// Edge `T = 0` is listed first.
pub fn parallel_holdout_window_candidates(
    duration_a: Duration,
    duration_b: Duration,
    segment_length: Duration,
) -> Vec<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return Vec::new();
    }

    let max_start = duration_a.as_secs_f64().min(duration_b.as_secs_f64()) - segment_secs;
    if max_start < 0.0 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push_unique = |start_secs: f64| {
        if start_secs < 0.0 || start_secs > max_start + 0.001 {
            return;
        }
        if candidates
            .iter()
            .any(|w: &ClipWindow| (w.start.as_secs_f64() - start_secs).abs() < 0.001)
        {
            return;
        }
        candidates.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(start_secs + segment_secs),
            ClipLabel::Interior,
        ));
    };

    push_unique(0.0);
    if max_start > 0.0 {
        push_unique(max_start);
    }

    candidates
}

/// Short hold-out segment centered inside a discovery clip window on A's timeline.
pub fn holdout_window_centered_in(
    window: &ClipWindow,
    segment_length: Duration,
) -> Option<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return None;
    }
    let win_start = window.start.as_secs_f64();
    let win_end = window.end.as_secs_f64();
    if win_end - win_start < segment_secs {
        return None;
    }
    let center = (win_start + win_end) / 2.0;
    let start_secs = (center - segment_secs / 2.0).clamp(win_start, win_end - segment_secs);
    Some(ClipWindow::new(
        secs_to_duration(start_secs),
        secs_to_duration(start_secs + segment_secs),
        window.label,
    ))
}

/// Hold-out candidates inside one discovery clip window, overlap-safe positions first.
///
/// Prefer near the window start (and overlap floor) before mid-window seeks that often fail
/// on MKV timestamp boundaries — same policy as [`holdout_window_candidates`].
pub fn anchor_holdout_candidates(
    discovery_window: &ClipWindow,
    segment_length: Duration,
    prior_offset_secs: f64,
    duration_a_secs: f64,
    duration_b_secs: f64,
) -> Vec<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return Vec::new();
    }
    let win_start = discovery_window.start.as_secs_f64();
    let win_end = discovery_window.end.as_secs_f64();
    if win_end - win_start < segment_secs {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push = |start_secs: f64| {
        if start_secs < win_start - 0.001 || start_secs + segment_secs > win_end + 0.001 {
            return;
        }
        if !holdout_window_feasible(
            start_secs,
            segment_secs,
            prior_offset_secs,
            duration_a_secs,
            duration_b_secs,
        ) {
            return;
        }
        if candidates
            .iter()
            .any(|window: &ClipWindow| (window.start.as_secs_f64() - start_secs).abs() < 0.001)
        {
            return;
        }
        candidates.push(ClipWindow::new(
            secs_to_duration(start_secs),
            secs_to_duration(start_secs + segment_secs),
            discovery_window.label,
        ));
    };

    let overlap_floor = (-prior_offset_secs).max(win_start);
    push(overlap_floor);

    let early = overlap_floor + 30.0;
    if early + segment_secs <= win_end {
        push(early);
    }

    push(win_start);

    if let Some(centered) = holdout_window_centered_in(discovery_window, segment_length) {
        push(centered.start.as_secs_f64());
    }

    push(win_end - segment_secs);

    candidates
}

pub fn holdout_window_feasible(
    window_start_secs: f64,
    segment_length_secs: f64,
    offset_secs: f64,
    duration_a_secs: f64,
    duration_b_secs: f64,
) -> bool {
    window_start_secs >= 0.0
        && window_start_secs + segment_length_secs <= duration_a_secs
        && window_start_secs + offset_secs >= 0.0
        && window_start_secs + segment_length_secs + offset_secs <= duration_b_secs
}

/// Map an A-timeline hold-out to the corresponding B window.
///
/// Returns `None` when the mapped range is negative or empty — callers must skip the candidate
/// instead of [`Duration::from_secs_f64`](Duration::from_secs_f64), which panics on negatives.
pub fn holdout_b_window_for_offset(
    holdout_on_a: &ClipWindow,
    segment_length: Duration,
    offset_secs: f64,
) -> Option<ClipWindow> {
    let segment_secs = segment_length.as_secs_f64();
    if segment_secs <= 0.0 {
        return None;
    }
    let b_start_secs = holdout_on_a.start.as_secs_f64() + offset_secs;
    let b_end_secs = b_start_secs + segment_secs;
    if b_start_secs < 0.0 || b_end_secs <= b_start_secs {
        return None;
    }
    Some(ClipWindow::new(
        secs_to_duration(b_start_secs),
        secs_to_duration(b_end_secs),
        holdout_on_a.label,
    ))
}

/// Timeline length used to decide whether a hold-out segment fits (symmetric vs query mode).
pub fn holdout_pick_duration(
    result: &AlignmentResult,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
) -> Duration {
    if result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference) {
        if let Some(loc) = &result.query_localization {
            if loc.skip_reason.is_none() {
                return Duration::from_secs_f64(loc.mapped_region.shared_length_secs.max(0.0));
            }
        }
    }
    extent_a.effective().min(extent_b.effective())
}

/// Hold-out candidates confined to a query-reference mapped region on A (absolute A time).
///
/// Discovery windows are rebased into `[0, shared_length)` on A, candidates are picked in
/// region-relative coordinates (offset `0` for placement), then shifted back to absolute A time.
/// Callers still validate each window with [`holdout_window_feasible`] using the global offset.
pub fn mapped_region_holdout_candidates(
    mapped: &TimelineOverlap,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
) -> Vec<ClipWindow> {
    let region_a_start = mapped.video_a_start_secs;
    let region_a_len_secs = mapped.shared_length_secs;
    let segment_secs = segment_length.as_secs_f64();
    if region_a_len_secs < segment_secs || segment_secs <= 0.0 {
        return Vec::new();
    }
    let region_duration = Duration::from_secs_f64(region_a_len_secs.max(0.0));

    let rebased: Vec<ClipWindow> = discovery_windows
        .iter()
        .map(|window| rebase_clip_window_to_region(*window, region_a_start, region_a_len_secs))
        .collect();

    let relative = holdout_window_candidates(region_duration, &rebased, segment_length, 0.0);

    relative
        .into_iter()
        .map(|window| shift_clip_window_on_a(window, region_a_start))
        .collect()
}

/// Hold-out placement for symmetric alignment or query-reference mapped-region mode.
pub fn resolve_holdout_candidates(
    result: &AlignmentResult,
    extent_a: MediaExtent,
    extent_b: MediaExtent,
    discovery_windows: &[ClipWindow],
    segment_length: Duration,
    offset_secs: f64,
) -> Vec<ClipWindow> {
    if result.alignment_mode_used == Some(AlignmentModeUsed::QueryReference) {
        if let Some(loc) = &result.query_localization {
            if loc.skip_reason.is_none() {
                return mapped_region_holdout_candidates(
                    &loc.mapped_region,
                    discovery_windows,
                    segment_length,
                );
            }
        }
    }

    let pick_duration = extent_a.effective().min(extent_b.effective());
    holdout_window_candidates(
        pick_duration,
        discovery_windows,
        segment_length,
        offset_secs,
    )
}

fn rebase_clip_window_to_region(
    window: ClipWindow,
    region_a_start: f64,
    region_a_len: f64,
) -> ClipWindow {
    let rel_start = (window.start.as_secs_f64() - region_a_start).clamp(0.0, region_a_len);
    let rel_end = (window.end.as_secs_f64() - region_a_start).clamp(rel_start, region_a_len);
    ClipWindow::new(
        secs_to_duration(rel_start),
        secs_to_duration(rel_end),
        window.label,
    )
}

fn shift_clip_window_on_a(window: ClipWindow, region_a_start: f64) -> ClipWindow {
    let shift = secs_to_duration(region_a_start);
    ClipWindow::new(window.start + shift, window.end + shift, window.label)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::alignment::TimelineOverlap;
    use crate::domain::clip_window::{ClipLabel, ClipWindow};
    use crate::domain::media_extent::MediaExtent;

    fn extent_secs(secs: f64) -> MediaExtent {
        MediaExtent::from_declared(Duration::from_secs_f64(secs))
    }

    #[test]
    fn pick_holdout_window_middle_for_single_clip() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(15),
            ClipLabel::Start,
        )];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert_eq!(window.start, Duration::from_secs(20));
        assert_eq!(window.end, Duration::from_secs(23));
    }

    #[test]
    fn pick_holdout_window_fits_two_clip_gap() {
        let discovery = vec![
            ClipWindow::new(Duration::ZERO, Duration::from_secs(15), ClipLabel::Start),
            ClipWindow::new(
                Duration::from_secs(45),
                Duration::from_secs(60),
                ClipLabel::End,
            ),
        ];
        let window =
            pick_holdout_window(Duration::from_secs(60), &discovery, Duration::from_secs(3))
                .unwrap();
        assert!((window.start.as_secs_f64() - 28.5).abs() < 0.001);
        assert!((window.end.as_secs_f64() - 31.5).abs() < 0.001);
    }

    #[test]
    fn pick_holdout_window_none_when_shorter_than_segment() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(2),
            ClipLabel::Start,
        )];
        assert!(
            pick_holdout_window(Duration::from_secs(2), &discovery, Duration::from_secs(3))
                .is_none()
        );
    }

    #[test]
    fn holdout_window_feasible_respects_offset() {
        assert!(holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 120.0));
        assert!(!holdout_window_feasible(10.0, 3.0, 3.0, 120.0, 12.0));
    }

    #[test]
    fn holdout_b_window_for_offset_rejects_negative_b_start() {
        let holdout = ClipWindow::new(
            Duration::from_secs_f64(8422.029),
            Duration::from_secs_f64(8425.029),
            ClipLabel::End,
        );
        assert!(holdout_b_window_for_offset(&holdout, Duration::from_secs(3), -8423.0).is_none());
        let mapped =
            holdout_b_window_for_offset(&holdout, Duration::from_secs(3), -100.0).expect("mapped");
        assert!((mapped.start.as_secs_f64() - 8322.029).abs() < 0.001);
    }

    #[test]
    fn holdout_window_centered_in_discovery_clip() {
        let window = ClipWindow::new(
            Duration::from_secs(100),
            Duration::from_secs(1000),
            ClipLabel::End,
        );
        let holdout = holdout_window_centered_in(&window, Duration::from_secs(3))
            .expect("hold-out should fit");
        assert!((holdout.start.as_secs_f64() - 548.5).abs() < 0.001);
        assert!((holdout.end.as_secs_f64() - 551.5).abs() < 0.001);
        assert_eq!(holdout.label, ClipLabel::End);
    }

    #[test]
    fn anchor_holdout_candidates_prefer_overlap_safe_window_start() {
        let start = ClipWindow::new(Duration::ZERO, Duration::from_secs(900), ClipLabel::Start);
        let candidates =
            anchor_holdout_candidates(&start, Duration::from_secs(3), -7.326, 7_547.0, 7_547.0);
        assert!(!candidates.is_empty());
        let first = candidates[0].start.as_secs_f64();
        assert!(
            first < 50.0,
            "first candidate should be near window start, got {first}"
        );
        assert!(
            !candidates
                .iter()
                .any(|window| { (window.start.as_secs_f64() - 448.5).abs() < 1.0 })
                || candidates[0].start.as_secs_f64() < 100.0,
            "centered mid-window seek should not be first choice"
        );

        let end = ClipWindow::new(
            Duration::from_secs(6_647),
            Duration::from_secs(7_547),
            ClipLabel::End,
        );
        let end_candidates =
            anchor_holdout_candidates(&end, Duration::from_secs(3), -6.674, 7_547.0, 7_547.0);
        assert!(!end_candidates.is_empty());
        assert!((end_candidates[0].start.as_secs_f64() - 6_647.0).abs() < 1.0);
    }

    #[test]
    fn high_rate_refine_skips_when_window_infeasible() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(60),
            ClipLabel::Start,
        )];
        let candidates = holdout_window_candidates(
            Duration::from_secs(60),
            &discovery,
            Duration::from_secs(3),
            3.0,
        );
        assert!(!candidates.is_empty());
        assert!(
            candidates.iter().all(|window| {
                !holdout_window_feasible(window.start.as_secs_f64(), 3.0, 3.0, 120.0, 5.0)
            }),
            "short B duration should make every candidate infeasible"
        );
    }

    #[test]
    fn holdout_candidates_include_early_fallback() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(600),
            ClipLabel::Start,
        )];
        let candidates = holdout_window_candidates(
            Duration::from_secs(203),
            &discovery,
            Duration::from_secs(3),
            0.0,
        );
        assert!(candidates.len() >= 2);
        assert!(candidates
            .iter()
            .any(|window| window.start.as_secs_f64() < 50.0));
    }

    #[test]
    fn holdout_candidates_prefer_overlap_safe_start_for_negative_offset() {
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(600),
            ClipLabel::Start,
        )];
        let candidates = holdout_window_candidates(
            Duration::from_secs(600),
            &discovery,
            Duration::from_secs(3),
            -11.019,
        );
        let first = candidates.first().expect("candidate");
        assert!((first.start.as_secs_f64() - 11.019).abs() < 0.001);
    }

    #[test]
    fn mapped_region_holdout_a_long_negative_offset_stays_in_region() {
        use crate::domain::query_localization::compute_mapped_region;

        let mapped = compute_mapped_region(2700.0, 480.0, extent_secs(3600.0), extent_secs(480.0));
        let discovery = vec![ClipWindow::new(
            Duration::from_secs(2640),
            Duration::from_secs(3120),
            ClipLabel::Start,
        )];
        let segment = Duration::from_secs(3);
        let candidates = mapped_region_holdout_candidates(&mapped, &discovery, segment);
        assert!(!candidates.is_empty());
        let region_end = mapped.video_a_start_secs + mapped.shared_length_secs;
        for window in &candidates {
            let start = window.start.as_secs_f64();
            let end = window.end.as_secs_f64();
            assert!(
                start >= mapped.video_a_start_secs - 0.001 && end <= region_end + 0.001,
                "window [{start}, {end}] outside mapped A [{}, {region_end}]",
                mapped.video_a_start_secs
            );
            assert!(holdout_window_feasible(start, 3.0, -2700.0, 3600.0, 480.0));
        }
    }

    #[test]
    fn mapped_region_holdout_b_long_positive_offset_stays_in_region() {
        use crate::domain::query_localization::compute_mapped_region;

        let pseudo = compute_mapped_region(2700.0, 480.0, extent_secs(3600.0), extent_secs(480.0));
        let mapped = TimelineOverlap {
            video_a_start_secs: pseudo.video_b_start_secs,
            video_a_end_secs: pseudo.video_b_end_secs,
            video_b_start_secs: pseudo.video_a_start_secs,
            video_b_end_secs: pseudo.video_a_end_secs,
            shared_length_secs: pseudo.shared_length_secs,
        };
        let discovery = vec![ClipWindow::new(
            Duration::ZERO,
            Duration::from_secs(420),
            ClipLabel::Start,
        )];
        let segment = Duration::from_secs(3);
        let candidates = mapped_region_holdout_candidates(&mapped, &discovery, segment);
        assert!(!candidates.is_empty());
        let region_end = mapped.shared_length_secs;
        for window in &candidates {
            let start = window.start.as_secs_f64();
            let end = window.end.as_secs_f64();
            assert!(
                start >= -0.001 && end <= region_end + 0.001,
                "window [{start}, {end}] outside mapped A [0, {region_end}]"
            );
            assert!(holdout_window_feasible(start, 3.0, 2700.0, 480.0, 3600.0));
        }
    }

    #[test]
    fn mapped_region_holdout_empty_when_region_shorter_than_segment() {
        let mapped = TimelineOverlap {
            video_a_start_secs: 100.0,
            video_a_end_secs: 102.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 2.0,
            shared_length_secs: 2.0,
        };
        let discovery = vec![ClipWindow::new(
            Duration::from_secs(100),
            Duration::from_secs(102),
            ClipLabel::Start,
        )];
        assert!(
            mapped_region_holdout_candidates(&mapped, &discovery, Duration::from_secs(3),)
                .is_empty()
        );
    }
}
