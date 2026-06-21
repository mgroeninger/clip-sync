use std::path::Path;

use clip_sync::{
    format_end_clip_anchor_line, format_high_rate_refinement_lines, format_offset_verification_lines,
    format_query_localization_lines, format_symmetric_clip_window_line, format_time_range,
    format_timestamp, ClipLabelReport,
};
use serde::Serialize;

use crate::application::error::RepairError;
use crate::application::patch_audio::PatchAudioResult;
use crate::application::ports::GapReporter;
use crate::domain::{
    CompatibilityVerdict, Gap, GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason,
    GapPatchStatus, GapReport, PatchSummary,
    diagnostics::collect_repair_warnings,
};
use crate::infrastructure::config::OutputFormat;

/// Gaps at or above this length get a duration marker when skipped or unfillable.
const LONG_GAP_SECS: f64 = 30.0;

pub struct StdoutGapReporter {
    pub format: OutputFormat,
}

impl GapReporter for StdoutGapReporter {
    fn report(&self, report: &GapReport) -> Result<(), RepairError> {
        match self.format {
            OutputFormat::Human => print_human(report),
            OutputFormat::Json => print_json(report),
        }
    }
}

fn format_human(
    report: &GapReport,
    patch: Option<&PatchSummary>,
    patch_result: Option<&PatchAudioResult>,
    show_diagnostics: bool,
    output_written: Option<&Path>,
) -> String {
    let mut out = String::new();
    let query_mode = report.alignment.query_localization.is_some();

    if query_mode {
        if let Some(loc) = &report.alignment.query_localization {
            for line in format_query_localization_lines(
                loc,
                report.alignment.recommended_offset_secs,
                show_diagnostics,
            ) {
                out.push_str(&line);
                out.push('\n');
            }
        }
    } else {
        let offset = report
            .alignment
            .recommended_offset_secs
            .map(|o| format!("{o:+.3}s"))
            .unwrap_or_else(|| "n/a (alignment failed)".into());
        let confidence = report
            .alignment
            .clips
            .first()
            .map(|c| format!("{:.2}", c.confidence))
            .unwrap_or_default();

        out.push_str(&format!("Alignment: offset {offset}  confidence {confidence}\n"));

        if report.alignment.clips.len() > 1 {
            for clip in &report.alignment.clips {
                let label = clip_label_name(clip.label);
                if let Some(clip_offset) = clip.offset_secs {
                    out.push_str(&format!(
                        "  {label} clip: {clip_offset:+.3}s  (confidence {:.2})\n",
                        clip.confidence
                    ));
                }
            }
        }
    }

    if !query_mode {
        if let Some(drift) = report.alignment.offset_drift_secs {
            if !report.alignment.offsets_consistent {
                out.push_str(&format!("Drift:     end − start = {drift:+.3}s\n"));
                if report.alignment.recommended_offset_secs.is_some() {
                    out.push_str(
                        "           using start-clip offset for fill (clip offsets disagree)\n",
                    );
                }
            }
        }
        if show_diagnostics {
            if let Some(anchor) = report.alignment.end_clip_anchor {
                out.push_str(&format!("{}\n", format_end_clip_anchor_line(anchor)));
            }
            if let Some(end) = report
                .alignment
                .clips
                .iter()
                .find(|clip| clip.label == ClipLabelReport::End)
            {
                out.push_str(&format!(
                    "  {}\n",
                    format_symmetric_clip_window_line(end, true)
                ));
            }
        }
    }

    if let Some(compat) = &report.track_compatibility {
        let verdict = match compat.verdict {
            CompatibilityVerdict::Identical => "identical",
            CompatibilityVerdict::Compatible => "compatible (resample B)",
            CompatibilityVerdict::Mismatch => "mismatch (fill blocked)",
        };
        out.push_str(&format!(
            "Tracks:    A {}ch @ {}Hz   B {}ch @ {}Hz   ({verdict})\n",
            compat.a_channels, compat.a_sample_rate, compat.b_channels, compat.b_sample_rate,
        ));
    } else {
        out.push_str("Tracks:    video B unavailable — compatibility not assessed\n");
    }

    if !query_mode {
        if let Some(overlap) = &report.overlap {
            out.push_str(&format!(
                "Overlap:   A [{:.2}s – {:.2}s]   B [{:.2}s – {:.2}s]   ({:.1}s shared)\n",
                overlap.video_a_start_secs,
                overlap.video_a_end_secs,
                overlap.video_b_start_secs,
                overlap.video_b_end_secs,
                overlap.shared_length_secs,
            ));
        }
    }

    if let Some(refine) = &report.alignment.high_rate_refinement {
        for line in format_high_rate_refinement_lines(refine, show_diagnostics) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if let Some(verify) = &report.alignment.offset_verification {
        for line in format_offset_verification_lines(verify, show_diagnostics) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if let Some(agreement) = &report.gap_offset_agreement {
        let verdict = if agreement.agrees { "AGREE" } else { "MISMATCH" };
        out.push_str(&format!(
            "Cross-chk: silence-based {:+.3}s vs alignment {:+.3}s  (Δ {:.3}s — {verdict})\n",
            agreement.silence_based_offset_secs,
            agreement.alignment_offset_secs,
            agreement.delta_secs,
        ));
        if !agreement.agrees {
            out.push_str(
                "           WARNING: silence structure disagrees with Chromaprint alignment\n",
            );
        }
    }

    if let Some(warning) = format_alignment_instability_warning(report, patch) {
        out.push_str(&warning);
    }

    let repair_warnings = collect_repair_warnings(
        report.overlap.as_ref().map(|o| o.video_a_start_secs),
        query_mode,
        report.audio_timeline_skew,
        patch_result.and_then(|r| r.pcm_container_skew),
    );
    for warning in &repair_warnings {
        out.push_str(warning);
        out.push('\n');
        tracing::warn!("{warning}");
    }

    out.push('\n');
    out.push_str(&format_unified_gap_report(report, patch, show_diagnostics));

    if let Some(path) = output_written {
        out.push_str(&format!("\nOutput: {}\n", path.display()));
    }

    out
}

pub fn format_unified_gap_report(
    report: &GapReport,
    patch: Option<&PatchSummary>,
    show_diagnostics: bool,
) -> String {
    if report.gaps.is_empty() {
        return "No gaps detected in video A.\n".into();
    }

    let mut out = String::new();
    out.push_str(&format_unified_gap_header(report, patch));
    if let Some(summary_line) = patch.and_then(|summary| format_patch_duration_summary(report, summary))
    {
        out.push_str(&summary_line);
    }
    if report.alignment.recommended_offset_secs.is_none() {
        out.push_str("  B timeline mapping skipped (no alignment offset).\n");
    }
    if patch.is_some_and(|summary| gap_table_uses_markers(report, summary)) {
        out.push_str("           (> skipped, - unfillable)\n");
    }
    out.push('\n');
    out.push_str("  #   Range                Dur      Status\n");

    for i in 0..report.gaps.len() {
        let gap = &report.gaps[i];
        let patch_outcome = patch.and_then(|summary| summary.gaps.get(i));
        let priority = gap_display_priority(patch_outcome);
        let status = format_unified_gap_status(gap, report, patch_outcome, show_diagnostics);
        out.push_str(&format!(
            "  {:<3} {:<20} {:<8} {status}\n",
            format_gap_row_index(priority, i + 1),
            format_time_range(gap.video_a_start_secs, gap.video_a_end_secs),
            format_gap_duration(gap.duration_secs(), priority),
        ));
    }

    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GapDisplayPriority {
    Skipped,
    NotPlanned,
    Patched,
    ScanOnly,
}

fn gap_display_priority(outcome: Option<&GapPatchOutcome>) -> GapDisplayPriority {
    match outcome.map(|o| &o.status) {
        Some(GapPatchStatus::Skipped { .. }) => GapDisplayPriority::Skipped,
        Some(GapPatchStatus::NotPlanned { .. }) => GapDisplayPriority::NotPlanned,
        Some(GapPatchStatus::Patched { .. }) => GapDisplayPriority::Patched,
        None => GapDisplayPriority::ScanOnly,
    }
}

fn gap_table_uses_markers(report: &GapReport, patch: &PatchSummary) -> bool {
    patch.gaps.iter().enumerate().any(|(i, outcome)| {
        let priority = gap_display_priority(Some(outcome));
        priority != GapDisplayPriority::Patched
            || report.gaps.get(i).is_some_and(|g| g.duration_secs() >= LONG_GAP_SECS)
    })
}

fn format_gap_row_index(priority: GapDisplayPriority, index: usize) -> String {
    match priority {
        GapDisplayPriority::Skipped => format!(">{index:<2}"),
        GapDisplayPriority::NotPlanned => format!("-{index:<2}"),
        _ => format!("{index:<3}"),
    }
}

fn format_gap_duration(duration_secs: f64, priority: GapDisplayPriority) -> String {
    let marker = if duration_secs >= LONG_GAP_SECS
        && matches!(
            priority,
            GapDisplayPriority::Skipped | GapDisplayPriority::NotPlanned
        )
    {
        "!"
    } else {
        ""
    };
    format!("{duration_secs:.1}s{marker}")
}

fn alignment_is_unstable(report: &GapReport) -> bool {
    if report.alignment.query_localization.is_some() {
        return false;
    }
    let drift = !report.alignment.offsets_consistent;
    let cross_mismatch = report
        .gap_offset_agreement
        .as_ref()
        .is_some_and(|agreement| !agreement.agrees);
    drift && cross_mismatch
}

fn notable_skipped_gap_refs(report: &GapReport, patch: &PatchSummary) -> Vec<usize> {
    let mut refs: Vec<(usize, f64)> = patch
        .gaps
        .iter()
        .enumerate()
        .filter_map(|(i, outcome)| {
            if matches!(outcome.status, GapPatchStatus::Skipped { .. }) {
                Some((i + 1, report.gaps.get(i).map(Gap::duration_secs).unwrap_or(0.0)))
            } else {
                None
            }
        })
        .collect();
    refs.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    refs.into_iter().map(|(index, _)| index).collect()
}

fn format_skipped_gap_hint(gap_refs: &[usize]) -> String {
    if gap_refs.is_empty() {
        return String::new();
    }
    if gap_refs.len() == 1 {
        format!(" (review gap #{})", gap_refs[0])
    } else {
        format!(
            " (review gaps #{})",
            gap_refs
                .iter()
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(", #")
        )
    }
}

fn format_alignment_instability_warning(
    report: &GapReport,
    patch: Option<&PatchSummary>,
) -> Option<String> {
    if !alignment_is_unstable(report) {
        return None;
    }
    let gap_hint = patch
        .map(|summary| format_skipped_gap_hint(&notable_skipped_gap_refs(report, summary)))
        .unwrap_or_default();
    Some(format!(
        "Warning:   alignment unstable — fills used start-clip offset; clip drift and silence cross-check disagree{gap_hint}\n"
    ))
}

fn format_patch_duration_summary(report: &GapReport, patch: &PatchSummary) -> Option<String> {
    if patch.patched_count == 0 && patch.skipped_count == 0 {
        return None;
    }

    let mut patched_secs = 0.0;
    let mut skipped_secs = 0.0;
    let mut longest_skip: Option<(usize, f64, f64)> = None;

    for (i, outcome) in patch.gaps.iter().enumerate() {
        let duration_secs = report.gaps.get(i).map(Gap::duration_secs).unwrap_or(0.0);
        match &outcome.status {
            GapPatchStatus::Patched { .. } => patched_secs += duration_secs,
            GapPatchStatus::Skipped { .. } => {
                skipped_secs += duration_secs;
                if longest_skip.is_none_or(|(_, dur, _)| duration_secs > dur) {
                    let start_secs = report.gaps.get(i).map(|g| g.video_a_start_secs).unwrap_or(0.0);
                    longest_skip = Some((i + 1, duration_secs, start_secs));
                }
            }
            GapPatchStatus::NotPlanned { .. } => {}
        }
    }

    let mut parts = Vec::new();
    if patch.patched_count > 0 {
        parts.push(format!("repaired {:.1}s of audio", patched_secs));
    }
    if patch.skipped_count > 0 {
        if let Some((index, _, start_secs)) = longest_skip {
            parts.push(format!(
                "skipped {:.1}s (gap #{index} at {})",
                skipped_secs,
                format_timestamp(start_secs)
            ));
        } else {
            parts.push(format!("skipped {:.1}s", skipped_secs));
        }
    }

    Some(format!("           {}\n", parts.join("; ")))
}

fn format_unified_gap_header(report: &GapReport, patch: Option<&PatchSummary>) -> String {
    let found = report.gaps.len();
    if let Some(summary) = patch {
        let marginal_note = if summary.patched_marginal_count > 0 {
            format!(" ({} marginal)", summary.patched_marginal_count)
        } else {
            String::new()
        };
        return format!(
            "Gaps in video A ({found} found, {} repaired{marginal_note}, {} skipped, {} unfillable):\n",
            summary.patched_count,
            summary.skipped_count,
            summary.not_planned_count,
        );
    }

    let repairable = report.repairable_count();
    let b_energy = report.fillable_count();
    if report.patch_allowed() {
        format!("Gaps in video A ({found} found, {repairable} repairable):\n")
    } else if b_energy > 0 {
        format!(
            "Gaps in video A ({found} found, 0 repairable — {b_energy} with B energy but fill blocked by track layout):\n"
        )
    } else {
        format!("Gaps in video A ({found} found, 0 repairable):\n")
    }
}

fn format_patch_slide_suffix(align_adjustment_secs: f64, waveform_adjustment_secs: f64) -> String {
    if waveform_adjustment_secs.abs() > 0.000_5 {
        format!(
            "slide={align_adjustment_secs:+.3}s (wf {waveform_adjustment_secs:+.3}s)"
        )
    } else {
        format!("slide={align_adjustment_secs:+.3}s")
    }
}

fn format_unified_gap_status(
    gap: &crate::domain::Gap,
    report: &GapReport,
    patch_outcome: Option<&crate::domain::GapPatchOutcome>,
    show_diagnostics: bool,
) -> String {
    let Some(outcome) = patch_outcome else {
        return gap_scan_status_label(gap, report).to_string();
    };

    match &outcome.status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            align_adjustment_secs,
            waveform_adjustment_secs,
            structure_trusted,
            confidence,
            ..
        } => {
            let slide = format_patch_slide_suffix(*align_adjustment_secs, *waveform_adjustment_secs);
            let marginal = if *confidence == crate::domain::FillConfidence::Marginal {
                "! "
            } else {
                ""
            };
            if show_diagnostics {
                if *structure_trusted {
                    format!(
                        "{marginal}patched (struct pre={pre_correlation:.2} post={post_correlation:.2} {slide})"
                    )
                } else {
                    format!(
                        "{marginal}patched (pre={pre_correlation:.2} post={post_correlation:.2} {slide})"
                    )
                }
            } else if *structure_trusted {
                format!("{marginal}patched (struct {pre_correlation:.2}→{post_correlation:.2})")
            } else {
                format!("{marginal}patched ({pre_correlation:.2}→{post_correlation:.2})")
            }
        }
        GapPatchStatus::Skipped { reason } => {
            format!("skipped: {}", format_patch_skip_reason(reason))
        }
        GapPatchStatus::NotPlanned { reason } => match reason {
            GapFillSkipReason::NotFillable => "unfillable".into(),
            GapFillSkipReason::OutsideReferenceCoverage => {
                "skipped (outside clip coverage)".into()
            }
            other => format!("not planned: {}", format_fill_skip_reason(other)),
        },
    }
}

fn gap_scan_status_label(gap: &crate::domain::Gap, report: &GapReport) -> &'static str {
    if !gap.is_fillable() {
        return "unfillable";
    }
    if report.limit_fill_to_mapped_region && report.gap_outside_reference_coverage(gap) {
        return "outside clip coverage";
    }
    if !report.patch_allowed() {
        return "blocked (track layout)";
    }
    "repairable"
}

fn clip_label_name(label: ClipLabelReport) -> &'static str {
    match label {
        ClipLabelReport::Start => "Start",
        ClipLabelReport::Interior => "Interior",
        ClipLabelReport::End => "End",
    }
}

fn print_human(report: &GapReport) -> Result<(), RepairError> {
    print!("{}", format_human(report, None, None, false, None));
    Ok(())
}

fn print_json(report: &GapReport) -> Result<(), RepairError> {
    print_json_with_patch(report, None)
}

#[derive(Serialize)]
struct RepairJsonOutput<'a> {
    scan: &'a GapReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    patch: Option<&'a PatchSummary>,
}

pub fn print_repair_output(
    report: &GapReport,
    patch: Option<&PatchSummary>,
    patch_result: Option<&PatchAudioResult>,
    format: OutputFormat,
    show_diagnostics: bool,
    output_written: Option<&Path>,
) -> Result<(), RepairError> {
    match format {
        OutputFormat::Human => {
            print!(
                "{}",
                format_human(report, patch, patch_result, show_diagnostics, output_written)
            );
            Ok(())
        }
        OutputFormat::Json => print_json_with_patch(report, patch),
    }
}

/// JSON repair report for stdout (`--format json`). Golden-tested in this module's tests.
pub fn format_repair_json_output(
    report: &GapReport,
    patch: Option<&PatchSummary>,
) -> Result<String, RepairError> {
    let payload = RepairJsonOutput { scan: report, patch };
    serde_json::to_string_pretty(&payload)
        .map_err(|e| RepairError::Config(format!("JSON serialization failed: {e}")))
}

fn print_json_with_patch(
    report: &GapReport,
    patch: Option<&PatchSummary>,
) -> Result<(), RepairError> {
    println!("{}", format_repair_json_output(report, patch)?);
    Ok(())
}

pub fn format_patch_summary(summary: &PatchSummary) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "\nPatch results ({} patched, {} skipped, {} not planned):\n",
        summary.patched_count, summary.skipped_count, summary.not_planned_count
    ));

    if summary.gaps.is_empty() {
        out.push_str("  (no gaps in scan report)\n");
        return out;
    }

    out.push('\n');

    for (i, gap) in summary.gaps.iter().enumerate() {
        let detail = match &gap.status {
            GapPatchStatus::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                waveform_adjustment_secs,
                structure_trusted,
                confidence,
                ..
            } => {
                let slide =
                    format_patch_slide_suffix(*align_adjustment_secs, *waveform_adjustment_secs);
                let marginal = if *confidence == crate::domain::FillConfidence::Marginal {
                    "! "
                } else {
                    ""
                };
                if *structure_trusted {
                    format!(
                        "{marginal}patched  (struct pre={pre_correlation:.2} post={post_correlation:.2} {slide})"
                    )
                } else {
                    format!(
                        "{marginal}patched  (pre={pre_correlation:.2} post={post_correlation:.2} {slide})"
                    )
                }
            }
            GapPatchStatus::Skipped { reason } => {
                format!("skipped: {}", format_patch_skip_reason(reason))
            }
            GapPatchStatus::NotPlanned { reason } => {
                format!("not planned: {}", format_fill_skip_reason(reason))
            }
        };
        out.push_str(&format!(
            "  #{:<3} [{:>8.2}s – {:>8.2}s]  ({:.1}s)  {}\n",
            i + 1,
            gap.a_start_secs,
            gap.a_end_secs,
            gap.a_end_secs - gap.a_start_secs,
            detail,
        ));
    }

    out
}

fn format_patch_skip_reason(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::BExtractFailed => "B audio extraction failed".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "boundary alignment failed".into(),
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
        } => format!(
            "boundary correlation below threshold (pre={pre_correlation:.2} post={post_correlation:.2} min={min_correlation:.2})"
        ),
        GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned B segment out of range".into(),
        GapPatchSkipReason::ZeroLengthGap => "zero-length gap".into(),
    }
}

fn format_fill_skip_reason(reason: &GapFillSkipReason) -> &'static str {
    match reason {
        GapFillSkipReason::NotFillable => "no B energy or alignment offset missing",
        GapFillSkipReason::TrackLayoutMismatch => "track layout mismatch",
        GapFillSkipReason::TrackCompatibilityUnavailable => "track compatibility unavailable",
        GapFillSkipReason::OutsideReferenceCoverage => "outside clip coverage",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{format_patch_summary, format_repair_json_output, RepairJsonOutput};
    use crate::domain::gap::{Gap, GapOffsetAgreement, GapReport};
    use crate::domain::{
        CompatibilityVerdict, GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason,
        GapPatchStatus, PatchSummary, TrackCompatibility,
    };
    use clip_sync::{
        AlignmentReport, AlignmentResult, ClipLabel, ClipMatch, ClipRepetitionReport,
        HighRateRefinement, OffsetVerification, RepetitionFinding, TimelineOverlap,
    };

    /// `include_str!` on Windows can embed CRLF from checkout; serde JSON uses LF.
    fn normalize_golden_newlines(s: &str) -> String {
        s.replace("\r\n", "\n")
    }

    fn full_surface_gap_report() -> GapReport {
        let overlap = TimelineOverlap {
            video_a_start_secs: 10.956,
            video_a_end_secs: 600.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 589.044,
            shared_length_secs: 589.044,
        };
        GapReport {
            video_a: PathBuf::from("video_a.mkv"),
            video_b: PathBuf::from("video_b.mkv"),
            track_compatibility: Some(TrackCompatibility {
                a_channels: 2,
                b_channels: 2,
                a_sample_rate: 48_000,
                b_sample_rate: 48_000,
                channels_match: true,
                rate_match: true,
                verdict: CompatibilityVerdict::Identical,
            }),
            overlap: Some(overlap.into()),
            alignment: AlignmentReport::from(&AlignmentResult {
                clips: vec![
                    ClipMatch {
                        label: ClipLabel::Start,
                        window_start_secs: 0.0,
                        window_end_secs: 900.0,
                        aligned: false,
                        offset_secs: None,
                        confidence: 0.42,
                        video_a_decode_skips: 1,
                        video_b_decode_skips: 2,
                        repetition: Some(ClipRepetitionReport {
                            a: Some(RepetitionFinding {
                                lag_secs: 30.5,
                                confidence: 0.72,
                                items_count: 48,
                            }),
                            b: None,
                        }),
                        video_b_window_start_secs: None,
                        video_b_window_end_secs: None,
                    },
                    ClipMatch {
                        label: ClipLabel::End,
                        window_start_secs: 1800.0,
                        window_end_secs: 2700.0,
                        aligned: true,
                        offset_secs: Some(12.355),
                        confidence: 0.91,
                        video_a_decode_skips: 0,
                        video_b_decode_skips: 3,
                        repetition: None,
                        video_b_window_start_secs: None,
                        video_b_window_end_secs: None,
                    },
                ],
                start_aligned: false,
                end_aligned: Some(true),
                recommended_offset_secs: Some(12.34),
                offsets_consistent: false,
                offset_drift_secs: Some(0.015),
                start_overlap: Some(overlap),
                high_rate_refinement: Some(HighRateRefinement {
                    segment_start_secs: 120.0,
                    segment_length_secs: 3.0,
                    adjustment_secs: 0.01,
                    correlation_peak: 2_813_101_397.0,
                    applied: true,
                    skipped: false,
                    skip_reason: None,
                    end_anchor: None,
                    refined_drift_secs: None,
                }),
                offset_verification: Some(OffsetVerification {
                    window_a_start_secs: 60.0,
                    window_a_end_secs: 90.0,
                    window_b_start_secs: 63.0,
                    window_b_end_secs: 93.0,
                    confidence: 0.85,
                    verified: true,
                    skipped: false,
                    skip_reason: None,
                    candidates_tried: 1,
                    independent_offset_secs: None,
                    parallel_recheck_delta_secs: None,
                    verify_inconclusive: false,
                }),
                offset_ambiguous_mod_secs: None,
                alignment_mode_used: None,
                query_localization: None,
                end_clip_anchor: Some(clip_sync::EndClipAnchor::SharedTimeline),
            }),
            gaps: vec![
                Gap {
                    video_a_start_secs: 45.0,
                    video_a_end_secs: 47.5,
                    video_b_start_secs: Some(57.34),
                    video_b_end_secs: Some(59.84),
                    b_has_energy: true,
                },
                Gap {
                    video_a_start_secs: 120.0,
                    video_a_end_secs: 125.0,
                    video_b_start_secs: None,
                    video_b_end_secs: None,
                    b_has_energy: false,
                },
            ],
            gap_offset_agreement: Some(GapOffsetAgreement {
                silence_based_offset_secs: 12.31,
                alignment_offset_secs: 12.34,
                delta_secs: 0.03,
                agrees: true,
            }),
            decode_chunk_secs: 10,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        }
    }

    fn full_surface_patch_summary() -> PatchSummary {
        PatchSummary::from_outcomes(vec![
            GapPatchOutcome {
                a_start_secs: 45.0,
                a_end_secs: 47.5,
                status: GapPatchStatus::Patched {
                    pre_correlation: 0.91,
                    post_correlation: 0.88,
                    align_adjustment_secs: 0.02,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: true,
                    confidence: crate::domain::FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                },
            },
            GapPatchOutcome {
                a_start_secs: 120.0,
                a_end_secs: 125.0,
                status: GapPatchStatus::Skipped {
                    reason: GapPatchSkipReason::CorrelationBelowThreshold {
                        pre_correlation: 0.22,
                        post_correlation: 0.19,
                        min_correlation: 0.35,
                    },
                },
            },
            GapPatchOutcome {
                a_start_secs: 200.0,
                a_end_secs: 205.0,
                status: GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable,
                },
            },
        ])
    }

    fn minimal_report() -> GapReport {
        let overlap = TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: 900.0,
            video_b_start_secs: 12.5,
            video_b_end_secs: 912.5,
            shared_length_secs: 900.0,
        };
        GapReport {
            video_a: PathBuf::from("a.mp4"),
            video_b: PathBuf::from("b.mp4"),
            track_compatibility: Some(TrackCompatibility {
                a_channels: 6,
                b_channels: 6,
                a_sample_rate: 48_000,
                b_sample_rate: 44_100,
                channels_match: true,
                rate_match: false,
                verdict: CompatibilityVerdict::Compatible,
            }),
            overlap: Some(overlap.into()),
            alignment: AlignmentReport::from(&AlignmentResult {
                clips: vec![ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: true,
                    offset_secs: Some(12.5),
                    confidence: 0.88,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                }],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: Some(12.5),
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: Some(overlap),
                high_rate_refinement: None,
                offset_verification: None,
                offset_ambiguous_mod_secs: None,
                alignment_mode_used: None,
                query_localization: None,
                end_clip_anchor: None,
            }),
            gaps: vec![Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 60.0,
                video_b_start_secs: Some(12.5),
                video_b_end_secs: Some(72.5),
                b_has_energy: true,
            }],
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        }
    }

    /// Regenerate `tests/fixtures/full_surface_repair.json` after an intentional contract change:
    /// `cargo test -p clip-sync-repair write_full_surface_repair_golden -- --ignored --nocapture`
    #[test]
    #[ignore = "fixture generator — run manually when the JSON contract is revised"]
    fn write_full_surface_repair_golden() {
        let report = full_surface_gap_report();
        let patch = full_surface_patch_summary();
        let json = format_repair_json_output(&report, Some(&patch)).expect("serialize");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/full_surface_repair.json");
        std::fs::create_dir_all(path.parent().expect("fixture parent dir")).expect("create fixtures dir");
        std::fs::write(&path, json).expect("write golden fixture");
    }

    /// Plan 1 Phase 0: byte-identical guard for the repair JSON contract (pre-DTO split).
    #[test]
    fn full_surface_repair_json_golden() {
        let report = full_surface_gap_report();
        let patch = full_surface_patch_summary();
        let json = format_repair_json_output(&report, Some(&patch)).expect("serialize");
        assert_eq!(
            normalize_golden_newlines(&json),
            normalize_golden_newlines(include_str!("../../../tests/fixtures/full_surface_repair.json")),
            "repair JSON contract changed — update the golden only with an explicit contract revision"
        );
    }

    #[test]
    fn json_report_is_valid_json() {
        let report = minimal_report();
        let json = serde_json::to_string(&report).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert!(value["gaps"].is_array());
        assert_eq!(value["gaps"][0]["b_has_energy"], true);
        assert_eq!(value["track_compatibility"]["verdict"], "compatible");
        assert_eq!(value["track_compatibility"]["channels_match"], true);
        assert_eq!(value["overlap"]["shared_length_secs"], 900.0);
    }

    #[test]
    fn json_repair_output_includes_patch_summary_when_present() {
        use crate::domain::{GapPatchOutcome, GapPatchStatus, PatchSummary};

        let report = minimal_report();
        let summary = PatchSummary::from_outcomes(vec![GapPatchOutcome {
            a_start_secs: 0.0,
            a_end_secs: 60.0,
            status: GapPatchStatus::Patched {
                pre_correlation: 0.91,
                post_correlation: 0.88,
                align_adjustment_secs: 0.02,
                waveform_adjustment_secs: 0.0,
                structure_trusted: false,
                confidence: crate::domain::FillConfidence::High,
                gap_start_adjust_frames: 0,
                gap_end_adjust_frames: 0,
            },
        }]);
        let payload = RepairJsonOutput {
            scan: &report,
            patch: Some(&summary),
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert!(value["scan"]["gaps"].is_array());
        assert_eq!(value["patch"]["patched_count"], 1);
        assert_eq!(
            value["patch"]["gaps"][0]["status"]["patched"]["pre_correlation"],
            0.91
        );
    }

    #[test]
    fn human_patch_summary_lists_patched_and_skipped_gaps() {
        use crate::domain::{
            GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
        };

        let summary = PatchSummary::from_outcomes(vec![
            GapPatchOutcome {
                a_start_secs: 1.0,
                a_end_secs: 4.0,
                status: GapPatchStatus::Patched {
                    pre_correlation: 0.92,
                    post_correlation: 0.90,
                    align_adjustment_secs: 0.01,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: true,
                    confidence: crate::domain::FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                },
            },
            GapPatchOutcome {
                a_start_secs: 5979.0,
                a_end_secs: 6180.0,
                status: GapPatchStatus::Skipped {
                    reason: GapPatchSkipReason::CorrelationBelowThreshold {
                        pre_correlation: 0.1,
                        post_correlation: 0.08,
                        min_correlation: 0.35,
                    },
                },
            },
            GapPatchOutcome {
                a_start_secs: 7000.0,
                a_end_secs: 7010.0,
                status: GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable,
                },
            },
        ]);

        let text = format_patch_summary(&summary);
        assert!(text.contains("1 patched, 1 skipped, 1 not planned"));
        assert!(text.contains("struct pre=0.92"));
        assert!(text.contains("skipped: boundary correlation below threshold"));
        assert!(text.contains("not planned: no B energy or alignment offset missing"));
    }

    fn failed_alignment_report() -> GapReport {
        GapReport {
            video_a: PathBuf::from("a.mp4"),
            video_b: PathBuf::from("b.mp4"),
            track_compatibility: None,
            overlap: None,
            alignment: AlignmentReport::from(&AlignmentResult {
                clips: vec![ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: false,
                    offset_secs: None,
                    confidence: 0.2,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                }],
                start_aligned: false,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                high_rate_refinement: None,
                offset_verification: None,
                offset_ambiguous_mod_secs: None,
                alignment_mode_used: None,
                query_localization: None,
                end_clip_anchor: None,
            }),
            gaps: vec![Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 60.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            }],
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        }
    }

    #[test]
    fn human_report_shows_drift_when_clips_disagree() {
        let mut report = minimal_report();
        report.alignment.clips = [
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(-10.956),
                confidence: 0.94,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: 5280.0,
                window_end_secs: 6180.0,
                aligned: true,
                offset_secs: Some(-11.2),
                confidence: 0.94,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
        ]
        .iter()
        .map(Into::into)
        .collect();
        report.alignment.end_aligned = Some(true);
        report.alignment.offsets_consistent = false;
        report.alignment.offset_drift_secs = Some(-0.244);
        report.alignment.recommended_offset_secs = Some(-10.956);

        let text = super::format_human(&report, None, None, false, None);
        assert!(text.contains("Start clip: -10.956s"));
        assert!(text.contains("End clip: -11.200s"));
        assert!(text.contains("Drift:"));
        assert!(text.contains("using start-clip offset for fill"));
    }

    #[test]
    fn human_report_renders_without_error() {
        super::print_human(&minimal_report()).expect("human render");
    }

    #[test]
    fn human_report_shows_cross_check_agreement() {
        use crate::domain::gap::GapOffsetAgreement;
        let mut report = minimal_report();
        report.gap_offset_agreement = Some(GapOffsetAgreement {
            silence_based_offset_secs: 12.48,
            alignment_offset_secs: 12.5,
            delta_secs: 0.02,
            agrees: true,
        });
        let text = super::format_human(&report, None, None, false, None);
        assert!(text.contains("Cross-chk"), "expected cross-check line");
        assert!(text.contains("AGREE"));
        assert!(!text.contains("WARNING"));
    }

    #[test]
    fn human_report_shows_cross_check_mismatch_warning() {
        use crate::domain::gap::GapOffsetAgreement;
        let mut report = minimal_report();
        report.gap_offset_agreement = Some(GapOffsetAgreement {
            silence_based_offset_secs: 7.0,
            alignment_offset_secs: 12.5,
            delta_secs: 5.5,
            agrees: false,
        });
        let text = super::format_human(&report, None, None, false, None);
        assert!(text.contains("MISMATCH"));
        assert!(text.contains("WARNING"));
    }

    #[test]
    fn human_failed_alignment_notes_b_mapping_skipped() {
        let text = super::format_human(&failed_alignment_report(), None, None, false, None);
        assert!(
            text.contains("B timeline mapping skipped"),
            "expected B mapping skipped note in human output"
        );
        assert!(text.contains("unfillable"));
    }

    #[test]
    fn json_failed_alignment_null_b_timeline_fields() {
        let report = failed_alignment_report();
        let json = serde_json::to_string(&report).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(value["alignment"]["recommended_offset_secs"], serde_json::Value::Null);
        assert_eq!(value["gaps"][0]["video_b_start_secs"], serde_json::Value::Null);
        assert_eq!(value["gaps"][0]["video_b_end_secs"], serde_json::Value::Null);
        assert_eq!(value["gaps"][0]["b_has_energy"], false);
    }

    #[test]
    fn human_report_marks_b_energy_gaps_blocked_when_tracks_mismatch() {
        use crate::domain::track_match::{CompatibilityVerdict, TrackCompatibility};

        let mut report = minimal_report();
        report.track_compatibility = Some(TrackCompatibility {
            a_channels: 6,
            b_channels: 2,
            a_sample_rate: 48_000,
            b_sample_rate: 48_000,
            channels_match: false,
            rate_match: true,
            verdict: CompatibilityVerdict::Mismatch,
        });
        report.gaps = vec![
            Gap {
                video_a_start_secs: 197.75,
                video_a_end_secs: 200.5,
                video_b_start_secs: Some(190.86),
                video_b_end_secs: Some(193.61),
                b_has_energy: true,
            },
            Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 7.25,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
        ];

        let text = super::format_human(&report, None, None, false, None);
        assert!(text.contains("0 repairable"));
        assert!(text.contains("fill blocked by track layout"));
        assert!(text.contains("blocked (track layout)"));
        assert!(text.contains("unfillable"));
    }

    #[test]
    fn unified_gap_report_merges_scan_and_patch() {
        use crate::domain::{GapPatchOutcome, GapPatchStatus, PatchSummary};

        let report = minimal_report();
        let summary = PatchSummary::from_outcomes(vec![GapPatchOutcome {
            a_start_secs: 0.0,
            a_end_secs: 60.0,
            status: GapPatchStatus::Patched {
                pre_correlation: 0.98,
                post_correlation: 1.0,
                align_adjustment_secs: 0.0,
                waveform_adjustment_secs: 0.0,
                structure_trusted: true,
                confidence: crate::domain::FillConfidence::High,
                gap_start_adjust_frames: 0,
                gap_end_adjust_frames: 0,
            },
        }]);

        let text = super::format_unified_gap_report(&report, Some(&summary), false);
        assert!(text.contains("1 found, 1 repaired, 0 skipped, 0 unfillable"));
        assert!(text.contains("patched (struct 0.98→1.00)"));
        assert!(!text.contains("Patch results"));
        assert!(!text.contains("Gaps detected"));
    }

    #[test]
    fn unified_gap_report_verbose_shows_patch_detail() {
        use crate::domain::{GapPatchOutcome, GapPatchStatus, PatchSummary};

        let report = minimal_report();
        let summary = PatchSummary::from_outcomes(vec![GapPatchOutcome {
            a_start_secs: 0.0,
            a_end_secs: 60.0,
            status: GapPatchStatus::Patched {
                pre_correlation: 0.92,
                post_correlation: 0.90,
                align_adjustment_secs: 0.01,
                waveform_adjustment_secs: 0.0,
                structure_trusted: true,
                confidence: crate::domain::FillConfidence::High,
                gap_start_adjust_frames: 0,
                gap_end_adjust_frames: 0,
            },
        }]);

        let text = super::format_unified_gap_report(&report, Some(&summary), true);
        assert!(text.contains("struct pre=0.92 post=0.90 slide=+0.010s"));
    }

    #[test]
    fn human_output_includes_written_path() {
        let report = minimal_report();
        let text = super::format_human(
            &report,
            None,
            None,
            false,
            Some(std::path::Path::new("out/repaired.mp4")),
        );
        assert!(text.contains("Output: out/repaired.mp4"));
    }

    #[test]
    fn human_report_shows_alignment_instability_warning() {
        use crate::domain::gap::GapOffsetAgreement;
        use crate::domain::{GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary};

        let mut report = minimal_report();
        report.gaps = vec![
            Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 2.0,
                video_b_start_secs: Some(12.5),
                video_b_end_secs: Some(14.5),
                b_has_energy: true,
            },
            Gap {
                video_a_start_secs: 6128.0,
                video_a_end_secs: 6359.7,
                video_b_start_secs: Some(6140.0),
                video_b_end_secs: Some(6371.7),
                b_has_energy: true,
            },
        ];
        report.alignment.clips = [
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(-4.853),
                confidence: 0.97,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: 5280.0,
                window_end_secs: 6180.0,
                aligned: true,
                offset_secs: Some(231.114),
                confidence: 0.93,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
        ]
        .iter()
        .map(Into::into)
        .collect();
        report.alignment.end_aligned = Some(true);
        report.alignment.offsets_consistent = false;
        report.alignment.offset_drift_secs = Some(235.966);
        report.alignment.recommended_offset_secs = Some(-4.853);
        report.alignment.end_clip_anchor = Some(clip_sync::EndClipAnchorReport::SharedTimeline);
        report.gap_offset_agreement = Some(GapOffsetAgreement {
            silence_based_offset_secs: 246.0,
            alignment_offset_secs: -4.853,
            delta_secs: 250.853,
            agrees: false,
        });

        let summary = PatchSummary::from_outcomes(vec![
            GapPatchOutcome {
                a_start_secs: 0.0,
                a_end_secs: 2.0,
                status: GapPatchStatus::Patched {
                    pre_correlation: 0.9,
                    post_correlation: 0.9,
                    align_adjustment_secs: 0.0,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: true,
                    confidence: crate::domain::FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                },
            },
            GapPatchOutcome {
                a_start_secs: 6128.0,
                a_end_secs: 6359.7,
                status: GapPatchStatus::Skipped {
                    reason: GapPatchSkipReason::BoundaryAlignmentFailed,
                },
            },
        ]);

        let text = super::format_human(&report, Some(&summary), None, false, None);
        assert!(text.contains("alignment unstable"));
        assert!(text.contains("review gap #2"));
        assert!(text.contains("repaired 2.0s of audio"));
        assert!(text.contains("skipped 231.7s (gap #2 at"));
        assert!(text.contains(">2 "));
        assert!(text.contains("231.7s!"));

        let verbose = super::format_human(&report, Some(&summary), None, true, None);
        assert!(verbose.contains("End anchor: shared timeline"));
        assert!(verbose.contains("End clip A"));
    }

    #[test]
    fn unified_gap_report_lists_gaps_in_timeline_order() {
        use crate::domain::{GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary};

        let mut report = minimal_report();
        report.gaps = vec![
            Gap {
                video_a_start_secs: 192.0,
                video_a_end_secs: 194.0,
                video_b_start_secs: Some(204.5),
                video_b_end_secs: Some(206.5),
                b_has_energy: true,
            },
            Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 8.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
            Gap {
                video_a_start_secs: 6128.0,
                video_a_end_secs: 6359.7,
                video_b_start_secs: Some(6140.0),
                video_b_end_secs: Some(6371.7),
                b_has_energy: true,
            },
        ];
        let summary = PatchSummary::from_outcomes(vec![
            GapPatchOutcome {
                a_start_secs: 192.0,
                a_end_secs: 194.0,
                status: GapPatchStatus::Patched {
                    pre_correlation: 0.9,
                    post_correlation: 0.9,
                    align_adjustment_secs: 0.0,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: true,
                    confidence: crate::domain::FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                },
            },
            GapPatchOutcome {
                a_start_secs: 0.0,
                a_end_secs: 8.0,
                status: GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable,
                },
            },
            GapPatchOutcome {
                a_start_secs: 6128.0,
                a_end_secs: 6359.7,
                status: GapPatchStatus::Skipped {
                    reason: GapPatchSkipReason::BoundaryAlignmentFailed,
                },
            },
        ]);

        let text = super::format_unified_gap_report(&report, Some(&summary), false);
        let patched_pos = text.find("patched (struct").expect("patched row");
        let unfillable_pos = text.find("-2 ").expect("unfillable marker");
        let skipped_pos = text.find(">3 ").expect("skipped marker");
        assert!(patched_pos < unfillable_pos);
        assert!(unfillable_pos < skipped_pos);
    }

    #[test]
    fn human_output_includes_repair_timeline_warnings() {
        use clip_sync::AudioTimelineSkew;

        let mut report = minimal_report();
        report.overlap = Some(clip_sync::TimelineOverlapReport {
            video_a_start_secs: 4.97,
            video_a_end_secs: 900.0,
            video_b_start_secs: 0.0,
            video_b_end_secs: 895.03,
            shared_length_secs: 895.03,
        });
        report.audio_timeline_skew = Some(AudioTimelineSkew {
            pts_secs: 0.0,
            sample_clock_secs: 4.9,
            delta_secs: 4.9,
        });

        let text = super::format_human(&report, None, None, false, None);
        assert!(text.contains("overlap starts at 5.0s"));
        assert!(text.contains("timeline mismatch on video A"));
    }

    #[test]
    fn gap_duration_secs() {
        let gap = Gap {
            video_a_start_secs: 100.0,
            video_a_end_secs: 160.0,
            video_b_start_secs: Some(112.5),
            video_b_end_secs: Some(172.5),
            b_has_energy: false,
        };
        assert!((gap.duration_secs() - 60.0).abs() < 0.001);
        assert!(!gap.is_fillable());
    }
}
