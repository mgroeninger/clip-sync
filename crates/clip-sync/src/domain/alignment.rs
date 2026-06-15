use std::time::Duration;

use crate::domain::ClipLabel;
use crate::domain::ClipWindow;

/// Chromaprint item sequence for one prepared mono clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    data: Vec<u32>,
}

impl Fingerprint {
    pub fn new(data: Vec<u32>) -> Self {
        Self { data }
    }

    pub fn items(&self) -> &[u32] {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Raw offset estimate from comparing one clip pair (video A vs video B).
///
/// `PartialEq` is derived for test convenience; float fields are not semantically exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipMatchEstimate {
    /// Seconds to add to video A's timeline to align with video B.
    pub offset_secs: f64,
    pub confidence: f32,
}

/// Internal repeat detected within a single prepared clip.
///
/// `PartialEq` is derived for test convenience; float fields are not semantically exact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepetitionFinding {
    /// Positive seconds between repeated content.
    pub lag_secs: f64,
    pub confidence: f32,
    pub items_count: usize,
}

/// Per-clip repetition diagnostics. Present on `ClipMatch` when `check_clip_repetition` was on.
/// JSON shape is owned by `application::report::RepetitionReport`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRepetitionReport {
    pub a: Option<RepetitionFinding>,
    pub b: Option<RepetitionFinding>,
}

/// Alignment outcome for a single clip pair at a known window position.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipMatch {
    pub label: ClipLabel,
    pub window_start_secs: f64,
    pub window_end_secs: f64,
    /// Whether the clip pair matched above the configured confidence threshold.
    pub aligned: bool,
    pub offset_secs: Option<f64>,
    pub confidence: f32,
    /// Corrupt decode packets skipped when extracting this clip from video A.
    pub video_a_decode_skips: u32,
    /// Corrupt decode packets skipped when extracting this clip from video B.
    pub video_b_decode_skips: u32,
    /// Present when `validation.check_clip_repetition` was on for this run.
    pub repetition: Option<ClipRepetitionReport>,
}

/// Shared timeline region implied by the start clip and recommended offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineOverlap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    pub shared_length_secs: f64,
}

/// Hold-out lag-0 check: verifies the recommended offset by comparing a shifted window at zero lag.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetVerification {
    pub window_a_start_secs: f64,
    pub window_a_end_secs: f64,
    pub window_b_start_secs: f64,
    pub window_b_end_secs: f64,
    /// Lag-0 fingerprint match confidence.
    pub confidence: f32,
    pub verified: bool,
    /// True when verification did not run (no feasible window, extract failure, etc.).
    pub skipped: bool,
    pub skip_reason: Option<String>,
    /// Hold-out windows scored before reporting (0 when skipped before any score).
    pub candidates_tried: u32,
    /// Calendar-parallel hold-out `find_offset` (same window on A and B); present when periodic recheck ran.
    pub independent_offset_secs: Option<f64>,
    /// `recommended_offset_secs - independent_offset_secs` when parallel recheck ran.
    pub parallel_recheck_delta_secs: Option<f64>,
    /// Option A scored a pass but periodic gating rejected it.
    pub verify_inconclusive: bool,
}

/// Native-rate hold-out FFT correction applied after discovery alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct HighRateRefinement {
    pub segment_start_secs: f64,
    pub segment_length_secs: f64,
    pub adjustment_secs: f64,
    pub correlation_peak: f64,
    pub applied: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

/// Full alignment report for all extracted clip pairs.
/// JSON shape is owned by `application::report::AlignmentReport`.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentResult {
    pub clips: Vec<ClipMatch>,
    pub start_aligned: bool,
    /// `None` when only one clip was extracted (no separate end window).
    pub end_aligned: Option<bool>,
    /// Best single offset when clips agree or when config prefers start/end.
    pub recommended_offset_secs: Option<f64>,
    /// All aligned clip pairs report the same offset (within tolerance).
    pub offsets_consistent: bool,
    /// End-clip offset minus start-clip offset when both clips aligned; diagnostic drift signal.
    pub offset_drift_secs: Option<f64>,
    /// Overlap on each file's timeline from the start clip match.
    pub start_overlap: Option<TimelineOverlap>,
    pub high_rate_refinement: Option<HighRateRefinement>,
    pub offset_verification: Option<OffsetVerification>,
    /// Repeat period **T** (seconds) when start-clip repetition makes the offset family ambiguous mod **T**.
    pub offset_ambiguous_mod_secs: Option<f64>,
}

impl AlignmentResult {
    /// Returns the clip with the given label, if present.
    pub fn clip_with_label(&self, label: ClipLabel) -> Option<&ClipMatch> {
        clip_with_label(&self.clips, label)
    }

    /// Returns the start-window clip, if present.
    pub fn start_clip(&self) -> Option<&ClipMatch> {
        self.clip_with_label(ClipLabel::Start)
    }
}

/// Returns the clip with the given label from a slice, if present.
pub fn clip_with_label<'a>(clips: &'a [ClipMatch], label: ClipLabel) -> Option<&'a ClipMatch> {
    clips.iter().find(|clip| clip.label == label)
}

/// Maximum start/end clip offset delta treated as agreement when merging estimates.
pub const OFFSET_AGREEMENT_TOLERANCE_SECS: f64 = 0.5;

/// Minimum internal repeat lag treated as a periodic ambiguity period.
pub const MIN_PERIODIC_REPEAT_LAG_SECS: f64 = 5.0;

/// Returns true when a repetition lag is close enough to the alignment offset that the
/// confidence estimate may be inflated by the repeated content.
pub fn should_downgrade_repetition_confidence(
    rep_a: &Option<RepetitionFinding>,
    rep_b: &Option<RepetitionFinding>,
    offset_secs: f64,
) -> bool {
    let close = |rep: &RepetitionFinding| (rep.lag_secs - offset_secs.abs()).abs() <= 1.0;
    rep_a.as_ref().is_some_and(close) || rep_b.as_ref().is_some_and(close)
}

fn is_strong_repetition_finding(finding: &RepetitionFinding, min_confidence: f32) -> bool {
    finding.confidence >= min_confidence && finding.lag_secs >= MIN_PERIODIC_REPEAT_LAG_SECS
}

/// Minimum whole periods a clip should contain when inferring a fundamental repeat from a harmonic lag.
const MIN_PERIODS_IN_CLIP_FOR_NORMALIZE: f64 = 3.0;

/// Reduces a detected repeat lag to the smallest plausible fundamental period **T** when the clip
/// contains several whole periods (e.g. 40 s harmonic → 10 s on a 60 s looped clip).
pub fn normalize_repeat_period(period_secs: f64, clip_duration_secs: f64) -> f64 {
    if period_secs <= MIN_PERIODIC_REPEAT_LAG_SECS
        || clip_duration_secs <= MIN_PERIODIC_REPEAT_LAG_SECS
        || period_secs <= 2.0 * MIN_PERIODIC_REPEAT_LAG_SECS
    {
        return period_secs;
    }

    let max_k = (period_secs / MIN_PERIODIC_REPEAT_LAG_SECS).floor() as i32;
    let mut best = period_secs;
    for k in 2..=max_k.max(2) {
        let candidate = period_secs / f64::from(k);
        if candidate < MIN_PERIODIC_REPEAT_LAG_SECS {
            continue;
        }
        if k > 1 && candidate < period_secs / 4.0 {
            continue;
        }
        let periods_in_clip = clip_duration_secs / candidate;
        if periods_in_clip >= MIN_PERIODS_IN_CLIP_FOR_NORMALIZE
            && (periods_in_clip - periods_in_clip.round()).abs() < 0.15
        {
            best = best.min(candidate);
        }
    }
    best
}

/// User-facing repeat period **T** from a raw autocorrelation lag and clip duration.
pub fn display_repeat_period(period_secs: f64, clip_duration_secs: f64) -> f64 {
    snap_suboctave_repeat_period(
        normalize_repeat_period(period_secs, clip_duration_secs),
        clip_duration_secs,
    )
}

/// When autocorrelation resolves a sub-multiple of the true tile (e.g. ~5 s for a 10 s loop),
/// promote to the doubled period when the clip contains whole periods at that scale.
fn snap_suboctave_repeat_period(period_secs: f64, clip_duration_secs: f64) -> f64 {
    let doubled = period_secs * 2.0;
    if period_secs >= 8.0 || doubled < MIN_PERIODIC_REPEAT_LAG_SECS {
        return period_secs;
    }
    let periods_in_clip = clip_duration_secs / doubled;
    if periods_in_clip >= MIN_PERIODS_IN_CLIP_FOR_NORMALIZE
        && (periods_in_clip - periods_in_clip.round()).abs() < 0.15
    {
        doubled
    } else {
        period_secs
    }
}

/// Best repeat period **T** from per-clip repetition diagnostics when either side has strong repeat.
pub fn periodic_ambiguity_period(
    report: &ClipRepetitionReport,
    min_repetition_confidence: f32,
    clip_duration_secs: Option<f64>,
) -> Option<f64> {
    let raw = match (&report.a, &report.b) {
        (Some(a), Some(b)) if is_strong_repetition_finding(a, min_repetition_confidence)
            && is_strong_repetition_finding(b, min_repetition_confidence) =>
        {
            Some(a.lag_secs.min(b.lag_secs))
        }
        (Some(a), _) if is_strong_repetition_finding(a, min_repetition_confidence) => Some(a.lag_secs),
        (_, Some(b)) if is_strong_repetition_finding(b, min_repetition_confidence) => Some(b.lag_secs),
        _ => None,
    };
    raw.map(|period| {
        let clip_secs = clip_duration_secs.unwrap_or(period);
        display_repeat_period(period, clip_secs)
    })
}

/// True when strong start-clip repetition makes the recommended offset a likely period alias, i.e.
/// the offset is large enough (`|offset| ≥ T − 1`) that it could be the true offset plus N×T rather
/// than the fundamental. Drives the discovery confidence downgrade so period aliases like +13 s
/// (T = 10) are penalized, while content aligned well inside the first period (offset ≈ 0, T = 30) is
/// not. See `docs/archive/periodic-ambiguity-plan.md`.
pub fn should_downgrade_periodic_ambiguity(
    report: &ClipRepetitionReport,
    min_repetition_confidence: f32,
    clip_duration_secs: Option<f64>,
    offset_secs: f64,
) -> bool {
    periodic_ambiguity_period(report, min_repetition_confidence, clip_duration_secs)
        .is_some_and(|period| offset_secs.abs() >= period - 1.0)
}

/// Rounds `recommended - independent` to the nearest integer multiple of `period_secs`.
/// Returns `None` when the residual exceeds [`OFFSET_AGREEMENT_TOLERANCE_SECS`].
pub fn periodic_recheck_period_multiple(
    recommended_secs: f64,
    independent_secs: f64,
    period_secs: f64,
) -> Option<i32> {
    if period_secs <= 0.0 {
        return None;
    }
    let diff = recommended_secs - independent_secs;
    let multiple = (diff / period_secs).round();
    let residual = (diff - multiple * period_secs).abs();
    if residual > OFFSET_AGREEMENT_TOLERANCE_SECS {
        return None;
    }
    Some(multiple as i32)
}

/// Sets [`AlignmentResult::offset_ambiguous_mod_secs`] from the start clip repetition report.
pub fn set_offset_ambiguous_mod_from_start_clip(
    result: &mut AlignmentResult,
    min_repetition_confidence: f32,
) {
    let Some(clip) = result.start_clip() else {
        return;
    };
    let clip_duration_secs = clip.window_end_secs - clip.window_start_secs;
    let Some(period) = clip
        .repetition
        .as_ref()
        .and_then(|report| {
            periodic_ambiguity_period(
                report,
                min_repetition_confidence,
                Some(clip_duration_secs),
            )
        })
    else {
        return;
    };
    result.offset_ambiguous_mod_secs = Some(period);
}

/// Per-clip alignment inputs for building an [`AlignmentResult`].
pub struct ClipPairReportInput<'a> {
    pub windows: &'a [ClipWindow],
    pub estimates: &'a [ClipMatchEstimate],
    pub decode_skips_a: &'a [u32],
    pub decode_skips_b: &'a [u32],
    pub duration_a: Option<Duration>,
    pub duration_b: Option<Duration>,
}

/// Policy for merging multi-clip estimates into a single recommendation.
pub struct AlignmentMergePolicy {
    pub min_match_score: f32,
    pub prefer_start_clip: bool,
    pub require_consistent_offsets: bool,
}

pub fn build_alignment_result(
    clips: ClipPairReportInput<'_>,
    policy: AlignmentMergePolicy,
) -> AlignmentResult {
    let ClipPairReportInput {
        windows,
        estimates,
        decode_skips_a,
        decode_skips_b,
        duration_a,
        duration_b,
    } = clips;
    let AlignmentMergePolicy {
        min_match_score,
        prefer_start_clip,
        require_consistent_offsets,
    } = policy;
    debug_assert_eq!(windows.len(), estimates.len());

    let clips: Vec<ClipMatch> = windows
        .iter()
        .zip(estimates.iter())
        .enumerate()
        .map(|(index, (window, estimate))| {
            let aligned = estimate.confidence >= min_match_score;
            ClipMatch {
                label: window.label,
                window_start_secs: duration_secs(window.start),
                window_end_secs: duration_secs(window.end),
                aligned,
                offset_secs: aligned.then_some(estimate.offset_secs),
                confidence: estimate.confidence,
                video_a_decode_skips: decode_skips_a.get(index).copied().unwrap_or(0),
                video_b_decode_skips: decode_skips_b.get(index).copied().unwrap_or(0),
                repetition: None,
            }
        })
        .collect();

    let start_aligned =
        clip_with_label(&clips, ClipLabel::Start).is_some_and(|clip| clip.aligned);

    let end_aligned = clip_with_label(&clips, ClipLabel::End).map(|clip| clip.aligned);

    let aligned_offsets: Vec<f64> = clips
        .iter()
        .filter_map(|clip| clip.offset_secs)
        .collect();

    let offsets_consistent =
        aligned_offsets.len() <= 1 || aligned_offsets.windows(2).all(|pair| {
            (pair[0] - pair[1]).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS
        });

    let offset_drift_secs = compute_offset_drift(&clips);

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
        offset_drift_secs,
        start_overlap,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
    }
}

pub fn refresh_start_overlap(
    result: &mut AlignmentResult,
    duration_a: Duration,
    duration_b: Duration,
) {
    result.start_overlap = compute_start_overlap(
        &result.clips,
        result.start_aligned,
        result.recommended_offset_secs,
        Some(duration_a),
        Some(duration_b),
    );
}

/// Timeline overlap implied by one aligned clip and its offset estimate.
pub fn compute_clip_timeline_overlap(
    clip: &ClipMatch,
    duration_a: Option<Duration>,
    duration_b: Option<Duration>,
) -> Option<TimelineOverlap> {
    if !clip.aligned {
        return None;
    }
    let offset = clip.offset_secs?;
    Some(compute_timeline_overlap(
        clip.window_start_secs,
        clip.window_end_secs,
        offset,
        duration_a,
        duration_b,
    ))
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
    let start = clip_with_label(clips, ClipLabel::Start)?;
    Some(compute_timeline_overlap(
        start.window_start_secs,
        start.window_end_secs,
        offset,
        duration_a,
        duration_b,
    ))
}

fn compute_timeline_overlap(
    window_start_secs: f64,
    window_end_secs: f64,
    offset: f64,
    duration_a: Option<Duration>,
    duration_b: Option<Duration>,
) -> TimelineOverlap {
    let a_dur = duration_a.map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY);
    let b_dur = duration_b.map(|d| d.as_secs_f64()).unwrap_or(f64::INFINITY);

    let t_lo = window_start_secs.max(-offset).max(0.0);
    let t_hi = window_end_secs.min(a_dur).min(b_dur - offset);

    if t_hi <= t_lo {
        return TimelineOverlap {
            video_a_start_secs: t_lo,
            video_a_end_secs: t_lo,
            video_b_start_secs: t_lo + offset,
            video_b_end_secs: t_lo + offset,
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

fn compute_offset_drift(clips: &[ClipMatch]) -> Option<f64> {
    let start = clip_with_label(clips, ClipLabel::Start)?.offset_secs?;
    let end = clip_with_label(clips, ClipLabel::End)?.offset_secs?;
    Some(end - start)
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
        clip_with_label(clips, label)
            .filter(|clip| clip.aligned)
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
    fn clip_with_label_finds_by_label() {
        let windows = vec![
            window(1800, 2700, ClipLabel::End),
            window(0, 900, ClipLabel::Start),
        ];
        let estimates = vec![
            ClipMatchEstimate {
                offset_secs: 12.0,
                confidence: 0.91,
            },
            ClipMatchEstimate {
                offset_secs: 12.0,
                confidence: 0.94,
            },
        ];
        let result = build_alignment_result(
            report_input(&windows, &estimates, None, None),
            default_policy(),
        );

        assert_eq!(result.clips[0].label, ClipLabel::End);
        assert_eq!(result.start_clip().unwrap().confidence, 0.94);
        assert_eq!(
            clip_with_label(&result.clips, ClipLabel::End)
                .unwrap()
                .confidence,
            0.91
        );
    }

    #[test]
    fn start_clip_returns_none_when_missing() {
        let result = AlignmentResult {
            clips: vec![ClipMatch {
                label: ClipLabel::Interior,
                window_start_secs: 0.0,
                window_end_secs: 60.0,
                aligned: true,
                offset_secs: Some(3.0),
                confidence: 0.8,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
            }],
            start_aligned: false,
            end_aligned: None,
            recommended_offset_secs: Some(3.0),
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
            offset_ambiguous_mod_secs: None,
        };
        assert!(result.start_clip().is_none());
    }

    #[test]
    fn periodic_recheck_period_multiple_cases() {
        assert_eq!(periodic_recheck_period_multiple(13.0, 3.0, 10.0), Some(1));
        assert_eq!(periodic_recheck_period_multiple(3.0, 3.0, 10.0), Some(0));
        assert_eq!(periodic_recheck_period_multiple(3.0, 5.0, 10.0), None);
    }

    #[test]
    fn periodic_ambiguity_period_prefers_smaller_strong_lag() {
        let report = ClipRepetitionReport {
            a: Some(RepetitionFinding {
                lag_secs: 10.0,
                confidence: 0.8,
                items_count: 40,
            }),
            b: Some(RepetitionFinding {
                lag_secs: 30.0,
                confidence: 0.6,
                items_count: 20,
            }),
        };
        assert_eq!(
            periodic_ambiguity_period(&report, 0.5, Some(60.0)),
            Some(10.0)
        );
    }

    #[test]
    fn normalize_repeat_period_reduces_harmonic_lag() {
        assert!((normalize_repeat_period(40.0, 60.0) - 10.0).abs() < 0.01);
        assert!((normalize_repeat_period(30.0, 65.0) - 30.0).abs() < 0.01);
    }

    #[test]
    fn snap_suboctave_repeat_period_promotes_five_second_harmonic() {
        assert!((snap_suboctave_repeat_period(5.0, 60.0) - 10.0).abs() < 0.1);
    }

    fn strong_repetition_report(lag_secs: f64) -> ClipRepetitionReport {
        ClipRepetitionReport {
            a: Some(RepetitionFinding {
                lag_secs,
                confidence: 0.9,
                items_count: 100,
            }),
            b: None,
        }
    }

    #[test]
    fn should_downgrade_periodic_ambiguity_period_alias_offsets() {
        let report = strong_repetition_report(10.0);
        let clip_secs = Some(60.0);
        assert!(
            should_downgrade_periodic_ambiguity(&report, 0.5, clip_secs, 13.0),
            "+13 s alias when T=10 should downgrade"
        );
        assert!(
            !should_downgrade_periodic_ambiguity(&report, 0.5, clip_secs, 3.0),
            "+3 s true offset inside first period should not downgrade"
        );
        assert!(
            should_downgrade_periodic_ambiguity(&report, 0.5, clip_secs, -13.0),
            "−13 s alias when T=10 should downgrade"
        );
        assert!(
            !should_downgrade_periodic_ambiguity(&report, 0.5, clip_secs, -3.0),
            "−3 s true offset should not downgrade"
        );
    }

    #[test]
    fn should_downgrade_periodic_ambiguity_boundary_at_period_minus_one() {
        let report = strong_repetition_report(10.0);
        assert!(should_downgrade_periodic_ambiguity(&report, 0.5, Some(60.0), 9.0));
        assert!(!should_downgrade_periodic_ambiguity(&report, 0.5, Some(60.0), 8.9));
    }

    #[test]
    fn should_downgrade_periodic_ambiguity_repeated_segment_offset_in_first_period() {
        let report = strong_repetition_report(30.0);
        let clip_secs = Some(65.0);
        assert!(
            periodic_ambiguity_period(&report, 0.5, clip_secs).is_some(),
            "strong repeat should still set periodic period"
        );
        assert!(
            !should_downgrade_periodic_ambiguity(&report, 0.5, clip_secs, 3.0),
            "repeated_segment_in_clip: +3 s with T≈30 s should not trigger periodic downgrade"
        );
    }

    #[test]
    fn should_downgrade_periodic_ambiguity_false_without_strong_repeat() {
        let report = ClipRepetitionReport {
            a: Some(RepetitionFinding {
                lag_secs: 10.0,
                confidence: 0.2,
                items_count: 100,
            }),
            b: None,
        };
        assert!(!should_downgrade_periodic_ambiguity(&report, 0.5, Some(60.0), 13.0));
    }

    #[test]
    fn fingerprint_accessors() {
        let fp = Fingerprint::new(vec![1, 2, 3]);
        assert_eq!(fp.len(), 3);
        assert!(!fp.is_empty());
        assert_eq!(fp.items(), &[1, 2, 3]);
        assert_eq!(fp, Fingerprint::new(vec![1, 2, 3]));
        assert!(Fingerprint::new(vec![]).is_empty());
    }

    fn report_input<'a>(
        windows: &'a [ClipWindow],
        estimates: &'a [ClipMatchEstimate],
        duration_a: Option<Duration>,
        duration_b: Option<Duration>,
    ) -> ClipPairReportInput<'a> {
        ClipPairReportInput {
            windows,
            estimates,
            decode_skips_a: &[],
            decode_skips_b: &[],
            duration_a,
            duration_b,
        }
    }

    fn default_policy() -> AlignmentMergePolicy {
        AlignmentMergePolicy {
            min_match_score: 0.5,
            prefer_start_clip: true,
            require_consistent_offsets: true,
        }
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
            report_input(
                &windows,
                &estimates,
                Some(Duration::from_secs(2700)),
                Some(Duration::from_secs(2700)),
            ),
            default_policy(),
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

        let result = build_alignment_result(report_input(&windows, &estimates, None, None), default_policy());
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

        let result = build_alignment_result(report_input(&windows, &estimates, None, None), default_policy());
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

        let result = build_alignment_result(report_input(&windows, &estimates, None, None), default_policy());
        assert!(!result.offsets_consistent);
        assert_eq!(result.recommended_offset_secs, None);
        assert!((result.offset_drift_secs.unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn recommends_start_when_inconsistent_and_consistency_not_required() {
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

        let mut policy = default_policy();
        policy.require_consistent_offsets = false;

        let result = build_alignment_result(report_input(&windows, &estimates, None, None), policy);
        assert!(!result.offsets_consistent);
        assert_eq!(result.recommended_offset_secs, Some(10.0));
        assert!((result.offset_drift_secs.unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn computes_overlap_for_each_aligned_clip() {
        let windows = vec![
            window(11, 900, ClipLabel::Start),
            window(1800, 2689, ClipLabel::End),
        ];
        let estimates = vec![
            ClipMatchEstimate {
                offset_secs: 11.0,
                confidence: 0.9,
            },
            ClipMatchEstimate {
                offset_secs: 11.0,
                confidence: 0.85,
            },
        ];

        let result = build_alignment_result(
            report_input(
                &windows,
                &estimates,
                Some(Duration::from_secs(2700)),
                Some(Duration::from_secs(2689)),
            ),
            default_policy(),
        );

        let start_overlap = compute_clip_timeline_overlap(
            &result.clips[0],
            Some(Duration::from_secs(2700)),
            Some(Duration::from_secs(2689)),
        )
        .expect("start clip overlap");
        assert_eq!(start_overlap.video_a_start_secs, 11.0);
        assert_eq!(start_overlap.video_a_end_secs, 900.0);
        assert_eq!(start_overlap.video_b_start_secs, 22.0);
        assert_eq!(start_overlap.video_b_end_secs, 911.0);

        let end_overlap = compute_clip_timeline_overlap(
            &result.clips[1],
            Some(Duration::from_secs(2700)),
            Some(Duration::from_secs(2689)),
        )
        .expect("end clip overlap");
        assert_eq!(end_overlap.video_a_start_secs, 1800.0);
        assert_eq!(end_overlap.video_a_end_secs, 2678.0);
        assert_eq!(end_overlap.video_b_start_secs, 1811.0);
        assert_eq!(end_overlap.video_b_end_secs, 2689.0);
    }

    #[test]
    fn overlap_clamps_to_shorter_video_b() {
        let windows = vec![window(0, 900, ClipLabel::Start)];
        let estimates = vec![ClipMatchEstimate {
            offset_secs: 12.0,
            confidence: 0.9,
        }];

        let result = build_alignment_result(
            report_input(
                &windows,
                &estimates,
                Some(Duration::from_secs(900)),
                Some(Duration::from_secs(850)),
            ),
            default_policy(),
        );

        let overlap = result.start_overlap.expect("expected overlap");
        assert_eq!(overlap.video_a_start_secs, 0.0);
        assert_eq!(overlap.video_a_end_secs, 838.0);
        assert_eq!(overlap.video_b_start_secs, 12.0);
        assert_eq!(overlap.video_b_end_secs, 850.0);
        assert_eq!(overlap.shared_length_secs, 838.0);
    }
}
