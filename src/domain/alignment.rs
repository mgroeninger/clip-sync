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
}

const OFFSET_AGREEMENT_TOLERANCE_SECS: f64 = 0.5;

pub fn build_alignment_result(
    windows: &[ClipWindow],
    estimates: &[ClipMatchEstimate],
    min_match_score: f32,
    prefer_start_clip: bool,
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
    );

    AlignmentResult {
        clips,
        start_aligned,
        end_aligned,
        recommended_offset_secs,
        offsets_consistent,
    }
}

fn choose_recommended_offset(
    clips: &[ClipMatch],
    aligned_offsets: &[f64],
    offsets_consistent: bool,
    prefer_start_clip: bool,
) -> Option<f64> {
    if aligned_offsets.is_empty() {
        return None;
    }

    if offsets_consistent {
        return Some(aligned_offsets[0]);
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

        let result = build_alignment_result(&windows, &estimates, 0.5, true);
        assert!(result.start_aligned);
        assert_eq!(result.end_aligned, Some(true));
        assert!(result.offsets_consistent);
        assert_eq!(result.recommended_offset_secs, Some(12.0));
        assert_eq!(result.clips.len(), 2);
    }

    #[test]
    fn reports_no_alignment_when_below_threshold() {
        let windows = vec![window(0, 900, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 5.0,
            confidence: 0.2,
        }];

        let result = build_alignment_result(&windows, &estimates, 0.5, true);
        assert!(!result.start_aligned);
        assert_eq!(result.end_aligned, None);
        assert_eq!(result.recommended_offset_secs, None);
    }

    #[test]
    fn single_clip_has_no_end_alignment_field() {
        let windows = vec![window(0, 60, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 1.0,
            confidence: 0.95,
        }];

        let result = build_alignment_result(&windows, &estimates, 0.5, true);
        assert_eq!(result.end_aligned, None);
        assert!(result.start_aligned);
    }
}
