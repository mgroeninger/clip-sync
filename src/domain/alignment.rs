use std::time::Duration;

use serde::Serialize;

use crate::domain::ClipLabel;
use crate::domain::ClipWindow;

#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    pub data: Vec<u32>,
}

/// Raw offset estimate from comparing one clip pair (video A vs video B).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipMatchEstimate {
    /// Seconds to add to video A's timeline to align with video B.
    pub offset_secs: f64,
    pub confidence: f32,
}

/// Alignment outcome for a single clip pair at a known window position.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipMatch {
    pub label: ClipLabel,
    pub window_start_secs: f64,
    pub window_end_secs: f64,
    /// Whether the clip pair matched above the configured confidence threshold.
    pub aligned: bool,
    pub offset_secs: Option<f64>,
    pub confidence: f32,
}

/// Shared timeline region implied by the start clip and recommended offset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TimelineOverlap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    pub shared_length_secs: f64,
}

/// Full alignment report for all extracted clip pairs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlignmentResult {
    pub clips: Vec<ClipMatch>,
    pub start_aligned: bool,
    /// `None` when only one clip was extracted (no separate end window).
    pub end_aligned: Option<bool>,
    /// Best single offset when clips agree or when config prefers start/end.
    pub recommended_offset_secs: Option<f64>,
    /// All aligned clip pairs report the same offset (within tolerance).
    pub offsets_consistent: bool,
    /// Overlap on each file's timeline from the start clip match.
    pub start_overlap: Option<TimelineOverlap>,
}

const OFFSET_AGREEMENT_TOLERANCE_SECS: f64 = 0.5;

pub fn build_alignment_result(
    windows: &[ClipWindow],
    estimates: &[ClipMatchEstimate],
    min_match_score: f32,
    prefer_start_clip: bool,
    require_consistent_offsets: bool,
    duration_a: Option<Duration>,
    duration_b: Option<Duration>,
) -> AlignmentResult {
    debug_assert_eq!(windows.len(), estimates.len());

    let clips: Vec<ClipMatch> = windows
        .iter()
        .zip(estimates.iter())
        .map(|(window, estimate)| {
            let aligned = estimate.confidence >= min_match_score;
            ClipMatch {
                label: window.label,
                window_start_secs: duration_secs(window.start),
                window_end_secs: duration_secs(window.end),
                aligned,
                offset_secs: aligned.then_some(estimate.offset_secs),
                confidence: estimate.confidence,
            }
        })
        .collect();

    let start_aligned = clips
        .iter()
        .find(|clip| clip.label == ClipLabel::Start)
        .is_some_and(|clip| clip.aligned);

    let end_aligned = clips
        .iter()
        .find(|clip| clip.label == ClipLabel::End)
        .map(|clip| clip.aligned);

    let aligned_offsets: Vec<f64> = clips
        .iter()
        .filter_map(|clip| clip.offset_secs)
        .collect();

    let offsets_consistent =
        aligned_offsets.len() <= 1 || aligned_offsets.windows(2).all(|pair| {
            (pair[0] - pair[1]).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS
        });

    let recommended_offset_secs = choose_recommended_offset(
        &clips,
        &aligned_offsets,
        offsets_consistent,
        prefer_start_clip,
        require_consistent_offsets,
    );

    let start_overlap = compute_start_overlap(
        &clips,
        start_aligned,
        recommended_offset_secs,
        duration_a,
        duration_b,
    );

    AlignmentResult {
        clips,
        start_aligned,
        end_aligned,
        recommended_offset_secs,
        offsets_consistent,
        start_overlap,
    }
}

fn compute_start_overlap(
    clips: &[ClipMatch],
    start_aligned: bool,
    recommended_offset_secs: Option<f64>,
    duration_a: Option<Duration>,
    duration_b: Option<Duration>,
) -> Option<TimelineOverlap> {
    if !start_aligned {
        return None;
    }
    let offset = recommended_offset_secs?;
    let start = clips.iter().find(|clip| clip.label == ClipLabel::Start)?;

    let a_dur = duration_a.map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY);
    let b_dur = duration_b.map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY);

    let t_lo = start
        .window_start_secs
        .max(-offset)
        .max(0.0);
    let t_hi = start
        .window_end_secs
        .min(a_dur)
        .min(b_dur - offset);

    if t_hi <= t_lo {
        return Some(TimelineOverlap {
            video_a_start_secs: t_lo,
            video_a_end_secs: t_lo,
            video_b_start_secs: t_lo + offset,
            video_b_end_secs: t_lo + offset,
            shared_length_secs: 0.0,
        });
    }

    Some(TimelineOverlap {
        video_a_start_secs: t_lo,
        video_a_end_secs: t_hi,
        video_b_start_secs: t_lo + offset,
        video_b_end_secs: t_hi + offset,
        shared_length_secs: t_hi - t_lo,
    })
}

fn choose_recommended_offset(
    clips: &[ClipMatch],
    aligned_offsets: &[f64],
    offsets_consistent: bool,
    prefer_start_clip: bool,
    require_consistent_offsets: bool,
) -> Option<f64> {
    if aligned_offsets.is_empty() {
        return None;
    }

    if !offsets_consistent && require_consistent_offsets {
        return None;
    }

    if let Some(fused) = weighted_offset_fusion(clips) {
        return Some(fused);
    }

    if offsets_consistent {
        return aligned_offsets.first().copied();
    }

    let pick = |label: ClipLabel| {
        clips
            .iter()
            .find(|clip| clip.label == label && clip.aligned)
            .and_then(|clip| clip.offset_secs)
    };

    if prefer_start_clip {
        pick(ClipLabel::Start)
            .or_else(|| pick(ClipLabel::End))
            .or_else(|| aligned_offsets.first().copied())
    } else {
        pick(ClipLabel::End)
            .or_else(|| pick(ClipLabel::Start))
            .or_else(|| aligned_offsets.last().copied())
    }
}

fn weighted_offset_fusion(clips: &[ClipMatch]) -> Option<f64> {
    let mut weighted: Vec<(f64, f32)> = clips
        .iter()
        .filter_map(|clip| clip.offset_secs.map(|offset| (offset, clip.confidence)))
        .collect();
    if weighted.is_empty() {
        return None;
    }

    let offsets: Vec<f64> = weighted.iter().map(|(offset, _)| *offset).collect();
    let median = median_offset(&offsets);
    weighted.retain(|(offset, _)| (*offset - median).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS);
    if weighted.is_empty() {
        return None;
    }

    let total_weight: f32 = weighted.iter().map(|(_, weight)| weight).sum();
    if total_weight <= 0.0 {
        return None;
    }

    Some(
        weighted
            .iter()
            .map(|(offset, weight)| offset * f64::from(weight / total_weight))
            .sum(),
    )
}

fn median_offset(offsets: &[f64]) -> f64 {
    let mut sorted = offsets.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

fn duration_secs(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::ClipWindow;

    fn window(start: u64, end: u64, label: ClipLabel) -> ClipWindow {
        ClipWindow::new(Duration::from_secs(start), Duration::from_secs(end), label)
    }

    #[test]
    fn reports_start_and_end_alignment_separately() {
        let windows = vec![
            window(0, 900, ClipLabel::Start),
            window(1800, 2700, ClipLabel::End),
        ];
        let estimates = vec![
            ClipMatchEstimate {
                offset_secs: 12.0,
                confidence: 0.9,
            },
            ClipMatchEstimate {
                offset_secs: 12.1,
                confidence: 0.85,
            },
        ];

        let result = build_alignment_result(
            &windows,
            &estimates,
            0.5,
            true,
            true,
            Some(Duration::from_secs(2700)),
            Some(Duration::from_secs(2700)),
        );
        assert!(result.start_aligned);
        assert_eq!(result.end_aligned, Some(true));
        assert!(result.offsets_consistent);
        assert!(
            (result.recommended_offset_secs.unwrap() - 12.05).abs() < 0.1,
            "expected weighted fusion near 12.05, got {:?}",
            result.recommended_offset_secs
        );
        assert_eq!(result.clips.len(), 2);

        let overlap = result.start_overlap.expect("expected overlap");
        assert_eq!(overlap.video_a_start_secs, 0.0);
        assert_eq!(overlap.video_a_end_secs, 900.0);
        let fused = result.recommended_offset_secs.unwrap();
        assert!((overlap.video_b_start_secs - fused).abs() < 0.01);
        assert!((overlap.video_b_end_secs - (900.0 + fused)).abs() < 0.01);
        assert_eq!(overlap.shared_length_secs, 900.0);
    }

    #[test]
    fn reports_no_alignment_when_below_threshold() {
        let windows = vec![window(0, 900, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 5.0,
            confidence: 0.2,
        }];

        let result = build_alignment_result(&windows, &estimates, 0.5, true, true, None, None);
        assert!(!result.start_aligned);
        assert_eq!(result.end_aligned, None);
        assert_eq!(result.recommended_offset_secs, None);
        assert!(result.start_overlap.is_none());
    }

    #[test]
    fn single_clip_has_no_end_alignment_field() {
        let windows = vec![window(0, 60, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 1.0,
            confidence: 0.95,
        }];

        let result = build_alignment_result(&windows, &estimates, 0.5, true, true, None, None);
        assert_eq!(result.end_aligned, None);
        assert!(result.start_aligned);
    }

    #[test]
    fn omits_recommendation_when_offsets_disagree_and_consistency_required() {
        let windows = vec![
            window(0, 900, ClipLabel::Start),
            window(1800, 2700, ClipLabel::End),
        ];
        let estimates = vec![
            ClipMatchEstimate {
                offset_secs: 10.0,
                confidence: 0.9,
            },
            ClipMatchEstimate {
                offset_secs: 20.0,
                confidence: 0.9,
            },
        ];

        let result = build_alignment_result(&windows, &estimates, 0.5, true, true, None, None);
        assert!(!result.offsets_consistent);
        assert_eq!(result.recommended_offset_secs, None);
    }

    #[test]
    fn overlap_clamps_to_shorter_video_b() {
        let windows = vec![window(0, 900, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 12.0,
            confidence: 0.9,
        }];

        let result = build_alignment_result(
            &windows,
            &estimates,
            0.5,
            true,
            true,
            Some(Duration::from_secs(900)),
            Some(Duration::from_secs(850)),
        );

        let overlap = result.start_overlap.expect("expected overlap");
        assert_eq!(overlap.video_a_start_secs, 0.0);
        assert_eq!(overlap.video_a_end_secs, 838.0);
        assert_eq!(overlap.video_b_start_secs, 12.0);
        assert_eq!(overlap.video_b_end_secs, 850.0);
        assert_eq!(overlap.shared_length_secs, 838.0);
    }
}
