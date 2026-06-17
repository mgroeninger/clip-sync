//! Serializable report DTOs for the JSON output contract.
//!
//! These types mirror the analyzer's JSON output field-for-field and decouple it from the
//! domain model: domain types carry no serde derives, and any domain change that should not
//! alter the JSON contract stops here. The authoritative field-by-field contract lives in
//! `docs/json-output.md`; byte-identical golden tests live in
//! `clip-sync-cli/tests/cli_output.rs` and `clip-sync-repair`'s output tests.

use serde::Serialize;

use crate::domain::{
    format_time_range, AlignmentModeUsed, AlignmentResult, ClipLabel, ClipMatch,
    ClipRepetitionReport, HighRateRefinement, OffsetVerification, QueryLocalization,
    RepetitionFinding, TimelineOverlap,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub independent_offset_secs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_recheck_delta_secs: Option<f64>,
    #[serde(skip_serializing_if = "is_false_bool")]
    pub verify_inconclusive: bool,
}

fn is_false_bool(value: &bool) -> bool {
    !*value
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
            independent_offset_secs: verify.independent_offset_secs,
            parallel_recheck_delta_secs: verify.parallel_recheck_delta_secs,
            verify_inconclusive: verify.verify_inconclusive,
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

/// How the run chose its algorithm (query-reference feature).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlignmentModeUsedReport {
    Symmetric,
    #[serde(rename = "queryreference")]
    QueryReference,
}

impl From<AlignmentModeUsed> for AlignmentModeUsedReport {
    fn from(mode: AlignmentModeUsed) -> Self {
        match mode {
            AlignmentModeUsed::Symmetric => Self::Symmetric,
            AlignmentModeUsed::QueryReference => Self::QueryReference,
        }
    }
}

/// Where a short query clip sits on the long reference timeline (query-reference mode).
///
/// Friendly `clip_on_a_*` / `clip_on_b_*` aliases mirror `mapped_region` for human-oriented
/// scripts; `recommended_offset_secs` / `start_overlap` on the parent report remain for tools.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryLocalizationReport {
    #[serde(alias = "anchor_a_secs")]
    pub anchor_ref_secs: f64,
    pub clip_on_a_start_secs: f64,
    pub clip_on_a_end_secs: f64,
    pub clip_on_b_start_secs: f64,
    pub clip_on_b_end_secs: f64,
    pub mapped_region: TimelineOverlapReport,
    pub search_stride_secs: f64,
    pub winning_window_start_secs: f64,
    pub winning_window_end_secs: f64,
    pub confidence: f32,
    pub ambiguous: bool,
    pub windows_scored: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

impl From<&QueryLocalization> for QueryLocalizationReport {
    fn from(loc: &QueryLocalization) -> Self {
        Self {
            anchor_ref_secs: loc.anchor_ref_secs,
            clip_on_a_start_secs: loc.clip_on_a_start_secs,
            clip_on_a_end_secs: loc.clip_on_a_end_secs,
            clip_on_b_start_secs: loc.clip_on_b_start_secs,
            clip_on_b_end_secs: loc.clip_on_b_end_secs,
            mapped_region: loc.mapped_region.into(),
            search_stride_secs: loc.search_stride_secs,
            winning_window_start_secs: loc.winning_window_start_secs,
            winning_window_end_secs: loc.winning_window_end_secs,
            confidence: loc.confidence,
            ambiguous: loc.ambiguous,
            windows_scored: loc.windows_scored,
            skip_reason: loc.skip_reason.clone(),
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
    /// Repeat period when start-clip repetition makes offset ambiguous mod **T**.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_ambiguous_mod_secs: Option<f64>,
    /// How this run chose its algorithm. Absent for the legacy symmetric path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment_mode_used: Option<AlignmentModeUsedReport>,
    /// Where the short clip sits on the long file. Present only in query-reference mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_localization: Option<QueryLocalizationReport>,
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
            offset_ambiguous_mod_secs: result.offset_ambiguous_mod_secs,
            alignment_mode_used: result.alignment_mode_used.map(Into::into),
            query_localization: result.query_localization.as_ref().map(Into::into),
        }
    }
}

/// Human-readable lines for high-rate refinement (CLI / repair reports).
pub fn format_high_rate_refinement_lines(
    refine: &HighRateRefinementReport,
    show_diagnostics: bool,
) -> Vec<String> {
    if refine.applied {
        let adjustment_secs = if refine.adjustment_secs == 0.0 {
            0.0
        } else {
            refine.adjustment_secs
        };
        if show_diagnostics {
            return vec![format!(
                "High-rate: {:+0.3}s refinement applied (peak {:.2})",
                adjustment_secs, refine.correlation_peak
            )];
        }
        return vec![format!(
            "High-rate: {:+0.3}s refinement applied",
            adjustment_secs
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
    if verify.verify_inconclusive {
        let mut lines = vec![format!(
            "Verify:    offset not independently verified (periodic content; hold-out confidence {:.2})",
            verify.confidence
        )];
        if show_diagnostics {
            if let Some(independent) = verify.independent_offset_secs {
                lines.push(format!(
                    "           parallel recheck offset {independent:+.3}s (Δ {:+.3}s vs recommended)",
                    verify.parallel_recheck_delta_secs.unwrap_or(0.0)
                ));
            }
        }
        return lines;
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

/// True when A is the longer (reference) file — mirrors [`crate::domain::query_reference_is_a`].
fn query_reference_is_a_report(loc: &QueryLocalizationReport) -> bool {
    (loc.anchor_ref_secs - loc.clip_on_a_start_secs).abs() < 1e-6
}

/// Human-readable lines for a query-reference localization.
///
/// Leads with *where the clip sits on the long file* (not offset/overlap jargon). When B is the
/// longer (donor) file, the default line adds the matching span on B. With `show_diagnostics`,
/// adds the B span (if not already shown), offset, and coarse-search stats for debugging.
pub fn format_query_localization_lines(
    loc: &QueryLocalizationReport,
    recommended_offset_secs: Option<f64>,
    show_diagnostics: bool,
) -> Vec<String> {
    if let Some(reason) = &loc.skip_reason {
        return vec![format!("Query clip not located ({reason})")];
    }

    let span = format_time_range(loc.clip_on_a_start_secs, loc.clip_on_a_end_secs);
    let clip_len_secs = (loc.clip_on_a_end_secs - loc.clip_on_a_start_secs).max(0.0);
    let b_is_reference = query_reference_is_a_report(loc);
    let mut match_line = format!(
        "Match on video A: {span}  ({}, confidence {:.2})",
        format_clip_length(clip_len_secs),
        loc.confidence
    );
    if !b_is_reference {
        let b_span = format_time_range(loc.clip_on_b_start_secs, loc.clip_on_b_end_secs);
        match_line.push_str(&format!("  (donor on B: {b_span})"));
    }
    let mut lines = vec![match_line];

    if loc.ambiguous {
        lines.push(
            "Warning:   clip location ambiguous (repeated content) — verify before trusting"
                .to_string(),
        );
    }

    if show_diagnostics {
        lines.push(format!(
            "Clip on B:  {}",
            format_time_range(loc.clip_on_b_start_secs, loc.clip_on_b_end_secs)
        ));
        if let Some(offset) = recommended_offset_secs {
            lines.push(format!(
                "Offset:     {:+.3}s  (add to A to align with B)",
                offset
            ));
        }
        lines.push(format!(
            "Search:     {} window(s) @ {:.0}s stride",
            loc.windows_scored, loc.search_stride_secs
        ));
    }

    lines
}

/// Compact clip-length label, e.g. `8m`, `1m30s`, `45s`.
fn format_clip_length(secs: f64) -> String {
    let total = secs.round() as i64;
    let minutes = total / 60;
    let seconds = total % 60;
    match (minutes, seconds) {
        (0, s) => format!("{s}s"),
        (m, 0) => format!("{m}m"),
        (m, s) => format!("{m}m{s}s"),
    }
}

/// Human-readable line when offset is ambiguous modulo a repeat period.
pub fn format_periodic_ambiguity_line(period_secs: f64) -> String {
    format!(
        "Warning:   offset ambiguous (repeats every ~{period_secs:.0} s) — auto offset and verify may match the wrong period"
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::testing::alignment_fixtures::{
        minimal_alignment_result, start_clip_match,
    };
    use crate::domain::{
        build_query_alignment_result, MediaExtent, QueryLocalization, ReferenceLocalizationOutcome,
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
                independent_offset_secs: None,
                parallel_recheck_delta_secs: None,
                verify_inconclusive: false,
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

    fn sample_localization() -> QueryLocalization {
        QueryLocalization::from_anchor(
            2700.0,
            480.0,
            crate::domain::MediaExtent::from_declared(std::time::Duration::from_secs(3600)),
            crate::domain::MediaExtent::from_declared(std::time::Duration::from_secs(480)),
            0.91,
            false,
            60.0,
            2640.0,
            3120.0,
            60,
        )
    }

    #[test]
    fn query_localization_report_round_trips_fields() {
        let report = QueryLocalizationReport::from(&sample_localization());
        assert!((report.anchor_ref_secs - 2700.0).abs() < 1e-9);
        assert!((report.clip_on_a_start_secs - 2700.0).abs() < 1e-9);
        assert!((report.clip_on_b_end_secs - 480.0).abs() < 1e-9);
        assert!((report.mapped_region.shared_length_secs - 480.0).abs() < 1e-9);
        assert_eq!(report.windows_scored, 60);
    }

    #[test]
    fn alignment_report_includes_query_fields_when_present() {
        let result = build_query_alignment_result(sample_localization(), 0.3);
        let report = AlignmentReport::from(&result);
        assert_eq!(
            report.alignment_mode_used,
            Some(AlignmentModeUsedReport::QueryReference)
        );
        assert!(report.query_localization.is_some());

        let value = serde_json::to_value(&report).expect("serialize");
        assert_eq!(value["alignment_mode_used"], "queryreference");
        assert_eq!(value["query_localization"]["clip_on_a_start_secs"], 2700.0);
        assert_eq!(value["query_localization"]["anchor_ref_secs"], 2700.0);
        assert!(value["query_localization"].get("anchor_a_secs").is_none());
    }

    #[test]
    fn query_localization_report_deserializes_anchor_a_secs_alias() {
        #[derive(serde::Deserialize)]
        struct AnchorField {
            #[serde(alias = "anchor_a_secs")]
            anchor_ref_secs: f64,
        }
        let parsed: AnchorField =
            serde_json::from_str(r#"{"anchor_a_secs":42.0}"#).expect("deserialize");
        assert!((parsed.anchor_ref_secs - 42.0).abs() < 1e-9);
    }

    #[test]
    fn alignment_report_omits_query_fields_for_symmetric() {
        let result = minimal_alignment_result(Some(3.0)).build();
        let report = AlignmentReport::from(&result);
        assert!(report.alignment_mode_used.is_none());
        assert!(report.query_localization.is_none());

        let value = serde_json::to_value(&report).expect("serialize");
        assert!(value.get("alignment_mode_used").is_none());
        assert!(value.get("query_localization").is_none());
    }

    #[test]
    fn format_query_lines_lead_with_placement() {
        let report = QueryLocalizationReport::from(&sample_localization());
        let lines = format_query_localization_lines(&report, Some(-2700.0), false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Match on video A: 45:00 – 53:00"));
        assert!(lines[0].contains("8m"));
        assert!(!lines[0].to_lowercase().contains("offset"));
    }

    #[test]
    fn format_query_lines_verbose_adds_offset_and_b_span() {
        let report = QueryLocalizationReport::from(&sample_localization());
        let lines = format_query_localization_lines(&report, Some(-2700.0), true);
        let joined = lines.join("\n");
        assert!(joined.contains("Clip on B:"));
        assert!(joined.contains("Offset:"));
        assert!(joined.contains("-2700.000s"));
    }

    #[test]
    fn format_query_lines_b_longer_default_includes_donor_on_b() {
        let loc = QueryLocalization::from_reference_outcome(
            ReferenceLocalizationOutcome {
                anchor_ref_secs: 240.0,
                query_duration_secs: 90.0,
                winning_window_start_secs: 180.0,
                winning_window_end_secs: 270.0,
                confidence: 0.91,
                ambiguous: false,
                windows_scored: 12,
                search_stride_secs: 60.0,
                skip_reason: None,
            },
            false,
            MediaExtent::from_declared(Duration::from_secs(90)),
            MediaExtent::from_declared(Duration::from_secs(360)),
        );
        let report = QueryLocalizationReport::from(&loc);
        let lines = format_query_localization_lines(&report, Some(240.0), false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("donor on B:"));
        assert!(lines[0].contains("4:00"));
        assert!(!lines[0].contains("Offset:"));
    }

    #[test]
    fn format_query_lines_verbose_b_longer_positive_offset() {
        let loc = QueryLocalization::from_reference_outcome(
            ReferenceLocalizationOutcome {
                anchor_ref_secs: 240.0,
                query_duration_secs: 90.0,
                winning_window_start_secs: 180.0,
                winning_window_end_secs: 270.0,
                confidence: 0.91,
                ambiguous: false,
                windows_scored: 12,
                search_stride_secs: 60.0,
                skip_reason: None,
            },
            false,
            MediaExtent::from_declared(Duration::from_secs(90)),
            MediaExtent::from_declared(Duration::from_secs(360)),
        );
        let report = QueryLocalizationReport::from(&loc);
        let lines = format_query_localization_lines(&report, Some(240.0), true);
        let joined = lines.join("\n");
        assert!(joined.contains("Offset:"));
        assert!(joined.contains("+240.000s"));
    }

    #[test]
    fn format_query_lines_skip_reason() {
        let loc = QueryLocalization::skipped("below threshold", 40, 60.0);
        let report = QueryLocalizationReport::from(&loc);
        let lines = format_query_localization_lines(&report, None, false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("below threshold"));
    }

    #[test]
    fn format_high_rate_refinement_uses_signed_adjustment() {
        use crate::domain::HighRateRefinement;

        let refine = HighRateRefinementReport::from(&HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs: 3.0,
            adjustment_secs: -0.0,
            correlation_peak: 1.0,
            applied: true,
            skipped: false,
            skip_reason: None,
        });
        let lines = format_high_rate_refinement_lines(&refine, false);
        assert_eq!(lines, vec!["High-rate: +0.000s refinement applied"]);

        let negative = HighRateRefinementReport::from(&HighRateRefinement {
            segment_start_secs: 0.0,
            segment_length_secs: 3.0,
            adjustment_secs: -0.012,
            correlation_peak: 1.0,
            applied: true,
            skipped: false,
            skip_reason: None,
        });
        let lines = format_high_rate_refinement_lines(&negative, false);
        assert_eq!(lines, vec!["High-rate: -0.012s refinement applied"]);
    }
}
