//! Serializable report DTOs for the JSON output contract.
//!
//! These types mirror the analyzer's JSON output field-for-field and decouple it from the
//! domain model: domain types carry no serde derives, and any domain change that should not
//! alter the JSON contract stops here. The authoritative field-by-field contract lives in
//! `docs/json-output.md`; byte-identical golden tests live in
//! `clip-sync-cli/tests/cli_output.rs` and `clip-sync-repair`'s output tests.

use serde::Serialize;

use crate::domain::{
    AlignmentResult, ClipLabel, ClipMatch, ClipRepetitionReport, HighRateRefinement,
    OffsetVerification, RepetitionFinding, TimelineOverlap,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipLabelReport {
    Start,
    Interior,
    End,
}

impl From<ClipLabel> for ClipLabelReport {
    fn from(label: ClipLabel) -> Self {
        match label {
            ClipLabel::Start => Self::Start,
            ClipLabel::Interior => Self::Interior,
            ClipLabel::End => Self::End,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RepetitionFindingReport {
    /// Positive seconds between repeated content.
    pub lag_secs: f64,
    pub confidence: f32,
    pub items_count: usize,
}

impl From<&RepetitionFinding> for RepetitionFindingReport {
    fn from(finding: &RepetitionFinding) -> Self {
        Self {
            lag_secs: finding.lag_secs,
            confidence: finding.confidence,
            items_count: finding.items_count,
        }
    }
}

/// Per-clip repetition diagnostics. `a`/`b` are `null` in JSON when no finding; the outer
/// `repetition` key is absent when the check was off (`skip_serializing_if` on
/// `ClipMatchReport.repetition`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepetitionReport {
    pub a: Option<RepetitionFindingReport>,
    pub b: Option<RepetitionFindingReport>,
}

impl From<&ClipRepetitionReport> for RepetitionReport {
    fn from(report: &ClipRepetitionReport) -> Self {
        Self {
            a: report.a.as_ref().map(Into::into),
            b: report.b.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipMatchReport {
    pub label: ClipLabelReport,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition: Option<RepetitionReport>,
}

impl From<&ClipMatch> for ClipMatchReport {
    fn from(clip: &ClipMatch) -> Self {
        Self {
            label: clip.label.into(),
            window_start_secs: clip.window_start_secs,
            window_end_secs: clip.window_end_secs,
            aligned: clip.aligned,
            offset_secs: clip.offset_secs,
            confidence: clip.confidence,
            video_a_decode_skips: clip.video_a_decode_skips,
            video_b_decode_skips: clip.video_b_decode_skips,
            repetition: clip.repetition.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TimelineOverlapReport {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    pub shared_length_secs: f64,
}

impl From<TimelineOverlap> for TimelineOverlapReport {
    fn from(overlap: TimelineOverlap) -> Self {
        Self {
            video_a_start_secs: overlap.video_a_start_secs,
            video_a_end_secs: overlap.video_a_end_secs,
            video_b_start_secs: overlap.video_b_start_secs,
            video_b_end_secs: overlap.video_b_end_secs,
            shared_length_secs: overlap.shared_length_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OffsetVerificationReport {
    pub window_a_start_secs: f64,
    pub window_a_end_secs: f64,
    pub window_b_start_secs: f64,
    pub window_b_end_secs: f64,
    /// Lag-0 fingerprint match confidence.
    pub confidence: f32,
    pub verified: bool,
    /// True when verification did not run (no feasible window, extract failure, etc.).
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_tried: Option<u32>,
}

impl From<&OffsetVerification> for OffsetVerificationReport {
    fn from(verify: &OffsetVerification) -> Self {
        Self {
            window_a_start_secs: verify.window_a_start_secs,
            window_a_end_secs: verify.window_a_end_secs,
            window_b_start_secs: verify.window_b_start_secs,
            window_b_end_secs: verify.window_b_end_secs,
            confidence: verify.confidence,
            verified: verify.verified,
            skipped: verify.skipped,
            skip_reason: verify.skip_reason.clone(),
            candidates_tried: (!verify.skipped && verify.candidates_tried > 0)
                .then_some(verify.candidates_tried),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HighRateRefinementReport {
    pub segment_start_secs: f64,
    pub segment_length_secs: f64,
    pub adjustment_secs: f64,
    pub correlation_peak: f64,
    pub applied: bool,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

impl From<&HighRateRefinement> for HighRateRefinementReport {
    fn from(refine: &HighRateRefinement) -> Self {
        Self {
            segment_start_secs: refine.segment_start_secs,
            segment_length_secs: refine.segment_length_secs,
            adjustment_secs: refine.adjustment_secs,
            correlation_peak: refine.correlation_peak,
            applied: refine.applied,
            skipped: refine.skipped,
            skip_reason: refine.skip_reason.clone(),
        }
    }
}

/// Full alignment report — the analyzer's JSON output payload (contract v1).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlignmentReport {
    pub clips: Vec<ClipMatchReport>,
    pub start_aligned: bool,
    /// `None` when only one clip was extracted (no separate end window).
    pub end_aligned: Option<bool>,
    /// Best single offset when clips agree or when config prefers start/end.
    pub recommended_offset_secs: Option<f64>,
    /// All aligned clip pairs report the same offset (within tolerance).
    pub offsets_consistent: bool,
    /// End-clip offset minus start-clip offset when both clips aligned; diagnostic drift signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_drift_secs: Option<f64>,
    /// Overlap on each file's timeline from the start clip match.
    pub start_overlap: Option<TimelineOverlapReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_rate_refinement: Option<HighRateRefinementReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_verification: Option<OffsetVerificationReport>,
}

impl From<&AlignmentResult> for AlignmentReport {
    fn from(result: &AlignmentResult) -> Self {
        Self {
            clips: result.clips.iter().map(Into::into).collect(),
            start_aligned: result.start_aligned,
            end_aligned: result.end_aligned,
            recommended_offset_secs: result.recommended_offset_secs,
            offsets_consistent: result.offsets_consistent,
            offset_drift_secs: result.offset_drift_secs,
            start_overlap: result.start_overlap.map(Into::into),
            high_rate_refinement: result.high_rate_refinement.as_ref().map(Into::into),
            offset_verification: result.offset_verification.as_ref().map(Into::into),
        }
    }
}

/// Human-readable lines for high-rate refinement (CLI / repair reports).
pub fn format_high_rate_refinement_lines(
    refine: &HighRateRefinementReport,
    show_diagnostics: bool,
) -> Vec<String> {
    if refine.applied {
        if show_diagnostics {
            return vec![format!(
                "High-rate: +{:.3}s refinement applied (peak {:.2})",
                refine.adjustment_secs, refine.correlation_peak
            )];
        }
        return vec![format!(
            "High-rate: +{:.3}s refinement applied",
            refine.adjustment_secs
        )];
    }
    if show_diagnostics {
        let reason = refine.skip_reason.as_deref().unwrap_or("not applied");
        return vec![format!("High-rate: skipped ({reason})")];
    }
    vec![]
}

/// Human-readable lines for hold-out offset verification.
pub fn format_offset_verification_lines(
    verify: &OffsetVerificationReport,
    show_diagnostics: bool,
) -> Vec<String> {
    if verify.skipped {
        if show_diagnostics {
            let reason = verify.skip_reason.as_deref().unwrap_or("unknown");
            return vec![format!("Verify:    skipped ({reason})")];
        }
        return vec![];
    }
    if !verify.verified {
        return vec![format!(
            "Verify:    offset not independently verified (hold-out confidence {:.2})",
            verify.confidence
        )];
    }
    if show_diagnostics {
        return vec![format!(
            "Verify:    offset confirmed at hold-out window (confidence {:.2})",
            verify.confidence
        )];
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::alignment_fixtures::{
        minimal_alignment_result, start_clip_match,
    };

    fn domain_clip(label: ClipLabel, repetition: Option<ClipRepetitionReport>) -> ClipMatch {
        let mut clip = start_clip_match(Some(3.0), 900.0, 0.9);
        clip.label = label;
        clip.video_a_decode_skips = 1;
        clip.video_b_decode_skips = 2;
        clip.repetition = repetition;
        clip
    }

    #[test]
    fn converts_all_none_optionals() {
        let result = minimal_alignment_result(Some(3.0))
            .with_clips(vec![domain_clip(ClipLabel::Start, None)])
            .build();

        let report = AlignmentReport::from(&result);
        assert_eq!(report.clips.len(), 1);
        assert_eq!(report.clips[0].label, ClipLabelReport::Start);
        assert!(report.clips[0].repetition.is_none());
        assert!(report.offset_drift_secs.is_none());
        assert!(report.start_overlap.is_none());
        assert!(report.high_rate_refinement.is_none());
        assert!(report.offset_verification.is_none());
    }

    #[test]
    fn converts_populated_optionals_and_repetition() {
        let mut result = minimal_alignment_result(Some(12.0))
            .with_clips(vec![domain_clip(
                ClipLabel::End,
                Some(ClipRepetitionReport {
                    a: Some(RepetitionFinding {
                        lag_secs: 30.5,
                        confidence: 0.7,
                        items_count: 4,
                    }),
                    b: None,
                }),
            )])
            .with_high_rate_refinement(Some(HighRateRefinement {
                segment_start_secs: 1.0,
                segment_length_secs: 3.0,
                adjustment_secs: 0.01,
                correlation_peak: 5.0,
                applied: true,
                skipped: false,
                skip_reason: None,
            }))
            .with_verification(Some(OffsetVerification {
                window_a_start_secs: 60.0,
                window_a_end_secs: 90.0,
                window_b_start_secs: 72.0,
                window_b_end_secs: 102.0,
                confidence: 0.8,
                verified: true,
                skipped: false,
                skip_reason: None,
                candidates_tried: 1,
            }))
            .build();
        result.start_aligned = false;
        result.end_aligned = Some(true);
        result.offsets_consistent = false;
        result.offset_drift_secs = Some(0.5);
        result.start_overlap = Some(TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 10.0,
            video_b_start_secs: 12.0,
            video_b_end_secs: 22.0,
            shared_length_secs: 10.0,
        });

        let report = AlignmentReport::from(&result);
        assert_eq!(report.clips[0].label, ClipLabelReport::End);
        let rep = report.clips[0].repetition.as_ref().expect("repetition");
        assert!((rep.a.expect("finding a").lag_secs - 30.5).abs() < 1e-9);
        assert!(rep.b.is_none());
        assert_eq!(report.end_aligned, Some(true));
        assert!((report.start_overlap.expect("overlap").shared_length_secs - 10.0).abs() < 1e-9);
        assert!(report.high_rate_refinement.expect("refinement").applied);
        assert!(report.offset_verification.expect("verification").verified);
    }

    #[test]
    fn clip_label_serializes_lowercase() {
        let json = serde_json::to_string(&ClipLabelReport::Interior).expect("serialize");
        assert_eq!(json, "\"interior\"");
    }
}
