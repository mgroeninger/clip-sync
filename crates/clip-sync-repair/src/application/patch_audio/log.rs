use clip_sync::{format_time_range_verbose, ProgressReporter};

use crate::domain::fill_mode::FillMode;
use crate::domain::fill_offset::FillOffsetMode;
use crate::domain::gap_fill::FillRegion;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::gap_tags::{format_gap_tags_verbose_line, GapTags};
use crate::domain::patch_result::{
    format_gap_fill_marginal_verbose_line, format_gap_fill_marginal_warn_reason,
    format_gap_fill_skip_verbose_line, format_gap_patch_skip_warn_reason, GapPatchSkipReason,
};

fn fill_offset_mode_label(mode: FillOffsetMode) -> &'static str {
    match mode {
        FillOffsetMode::Recommended => "recommended",
        FillOffsetMode::Interpolated => "interpolated",
        FillOffsetMode::AnchoredRetry => "anchored_retry",
    }
}

/// Per-gap A/B timeline fields for verbose fill planning logs.
pub(crate) struct GapFillPlanLog<'a> {
    pub scan_a_start_secs: f64,
    pub scan_a_end_secs: f64,
    pub refined_a_start_secs: f64,
    pub refined_a_end_secs: f64,
    pub gap_offset_secs: f64,
    pub fill_offset_mode: FillOffsetMode,
    pub mapped_b_start_secs: f64,
    pub mapped_b_end_secs: f64,
    pub b_search_start_secs: f64,
    pub b_search_end_secs: f64,
    pub signature_mode_label: &'a str,
}

/// B fill placement and slide metadata for verbose result logs.
pub(crate) struct GapFillResultLog {
    pub b_search_start_secs: f64,
    pub sample_rate: u32,
    pub channels: usize,
    pub fill_start_sample: usize,
    pub fill_end_sample: usize,
    pub structure_slide_secs: f64,
    pub waveform_slide_secs: f64,
    pub fit_used_boundary_grid: bool,
    pub fit_boundary_grid_cells: Option<u32>,
    pub fit_haystack_secs: f64,
    pub report_pre: f64,
    pub report_post: f64,
    pub confidence: FillConfidence,
}

/// Verbose stderr lines: per-gap A/B timeline used for structure search and fill.
pub(crate) fn format_gap_fill_plan_lines(plan: &GapFillPlanLog<'_>) -> Vec<String> {
    let mut lines = vec![format!(
        "           fill offset {:+.3}s ({})",
        plan.gap_offset_secs,
        fill_offset_mode_label(plan.fill_offset_mode),
    )];
    if (plan.refined_a_start_secs - plan.scan_a_start_secs).abs() > 0.001
        || (plan.refined_a_end_secs - plan.scan_a_end_secs).abs() > 0.001
    {
        lines.push(format!(
            "           A gap (refined): {}",
            format_time_range_verbose(plan.refined_a_start_secs, plan.refined_a_end_secs)
        ));
    }
    lines.push(format!(
        "           B gap (mapped): {}",
        format_time_range_verbose(plan.mapped_b_start_secs, plan.mapped_b_end_secs)
    ));
    lines.push(format!(
        "           B search window: {}",
        format_time_range_verbose(plan.b_search_start_secs, plan.b_search_end_secs)
    ));
    lines.push(format!(
        "           signature_mode={}",
        plan.signature_mode_label
    ));
    lines
}

pub(crate) fn format_gap_fill_result_line(result: &GapFillResultLog) -> String {
    let ch = result.channels.max(1);
    let to_secs = |sample: usize| sample as f64 / ch as f64 / f64::from(result.sample_rate);
    let fill_start = result.b_search_start_secs + to_secs(result.fill_start_sample);
    let fill_end = result.b_search_start_secs + to_secs(result.fill_end_sample);
    let mut slide = format!("structure slide {:+.3}s", result.structure_slide_secs);
    if result.waveform_slide_secs.abs() > 0.000_5 {
        slide.push_str(&format!(
            ", waveform slide {:+.3}s",
            result.waveform_slide_secs
        ));
    }
    let fit_path = if result.fit_used_boundary_grid {
        if let Some(cells) = result.fit_boundary_grid_cells {
            format!(
                "boundary grid ({cells} cells, haystack {:.1}s)",
                result.fit_haystack_secs
            )
        } else {
            "boundary grid".to_string()
        }
    } else if result.confidence == FillConfidence::Marginal {
        format!(
            "baseline only (marginal, pre={:.2} post={:.2})",
            result.report_pre, result.report_post
        )
    } else {
        "baseline only".to_string()
    };
    format!(
        "           B fill source: {} ({slide}; fit path: {fit_path})",
        format_time_range_verbose(fill_start, fill_end),
    )
}

pub(super) fn log_gap_fill_plan_verbose(
    progress: &dyn ProgressReporter,
    plan: &GapFillPlanLog<'_>,
) {
    for line in format_gap_fill_plan_lines(plan) {
        progress.phase_verbose(&line);
    }
}

pub(super) fn log_gap_fill_result_verbose(
    progress: &dyn ProgressReporter,
    result: &GapFillResultLog,
) {
    progress.phase_verbose(&format_gap_fill_result_line(result));
}

/// Verbose per-gap header for pass 1 (characterize).
///
/// Carries both axes, deliberately as two tokens: `gap #{n}` is the *identity* (the report table's
/// `#`, from [`FillRegion::gap_index`]) and `{k} of {m} planned` is the *progress count* over
/// `GapFillPlan.regions`. They diverge whenever a gap is skipped at plan time, which is why they are
/// never fused into one number.
pub(crate) fn format_patch_characterize_verbose_line(
    gap_index: usize,
    region_num: u64,
    region_count: u64,
    a_start_secs: f64,
    a_end_secs: f64,
) -> String {
    format!(
        "  gap #{} ({region_num} of {region_count} planned): A {}",
        gap_index + 1,
        format_time_range_verbose(a_start_secs, a_end_secs)
    )
}

/// Verbose per-gap header for the anchored retry pass.
///
/// Identity only — no `k of m`. Pass 2 iterates a *filtered* retry subset, so a count here would be a
/// position within a set the user never sees listed.
pub(crate) fn format_anchored_retry_verbose_line(
    retry_label: &str,
    gap_index: usize,
    a_start_secs: f64,
    a_end_secs: f64,
) -> String {
    format!(
        "  anchored {retry_label} gap #{}: A {}",
        gap_index + 1,
        format_time_range_verbose(a_start_secs, a_end_secs)
    )
}

/// The one construction site for the `patch_gap` tracing span.
///
/// Both passes go through here so the field set cannot drift — in particular `region_index`
/// (1-based ordinal within `GapFillPlan.regions`) and `gap_index` (0-based position in
/// `GapReport.gaps`) stay distinct fields rather than collapsing into one ambiguous number.
/// `record_patch_gap_span` fills the `Empty` fields once the outcome is known.
pub(super) fn new_patch_gap_span(
    region_num: u64,
    region_count: u64,
    region: &FillRegion,
    fill_mode: FillMode,
    anchored_retry: bool,
) -> tracing::Span {
    tracing::info_span!(
        "patch_gap",
        region_index = region_num,
        gap_index = region.gap_index,
        region_count,
        a_start_secs = region.a_start_secs,
        a_end_secs = region.a_end_secs,
        fill_mode = ?fill_mode,
        anchored_retry,
        outcome = tracing::field::Empty,
        confidence = tracing::field::Empty,
        skip_reason = tracing::field::Empty,
        boundary_grid = tracing::field::Empty,
        grid_cells = tracing::field::Empty,
    )
}

/// Human-readable skip line for stderr (`tracing::warn`) matching the stdout gap table.
///
/// `gap_index` is the region's [`FillRegion::gap_index`] — the gap's 0-based position in the report,
/// rendered as the table's 1-based `#`. This is an *identity*, never a progress count, so it carries
/// no `/total` (see the gap-index convention: identity and count are never one token).
pub(crate) fn format_skip_gap_fill_log(
    gap_index: usize,
    a_start_secs: f64,
    a_end_secs: f64,
    reason: &str,
) -> String {
    let range = format_time_range_verbose(a_start_secs, a_end_secs);
    format!("gap #{} ({range}): {reason}", gap_index + 1)
}

pub(super) fn log_skip_gap_fill(
    progress: &dyn ProgressReporter,
    region: &FillRegion,
    reason: &GapPatchSkipReason,
) {
    progress.flush_progress();
    if progress.detailed_extraction_progress() {
        progress.phase_verbose(&format_gap_fill_skip_verbose_line(reason));
    } else {
        tracing::warn!(
            "{}",
            format_skip_gap_fill_log(
                region.gap_index,
                region.a_start_secs,
                region.a_end_secs,
                &format_gap_patch_skip_warn_reason(reason),
            )
        );
    }
}

pub(super) struct MarginalGapFillLog {
    /// 0-based position in `GapReport.gaps`; see [`FillRegion::gap_index`].
    pub(super) gap_index: usize,
    pub(super) a_start_secs: f64,
    pub(super) a_end_secs: f64,
    pub(super) pre: f64,
    pub(super) post: f64,
    pub(super) min: f32,
    pub(super) anchor_seam: bool,
}

pub(super) fn log_marginal_gap_fill(progress: &dyn ProgressReporter, log: &MarginalGapFillLog) {
    progress.flush_progress();
    if progress.detailed_extraction_progress() {
        progress.phase_verbose(&format_gap_fill_marginal_verbose_line(
            log.pre,
            log.post,
            log.min,
            log.anchor_seam,
        ));
    } else {
        tracing::warn!(
            "{}",
            format_skip_gap_fill_log(
                log.gap_index,
                log.a_start_secs,
                log.a_end_secs,
                &format_gap_fill_marginal_warn_reason(log.pre, log.post, log.min, log.anchor_seam,),
            )
        );
    }
}

pub(super) fn log_gap_tags_verbose(progress: &dyn ProgressReporter, tags: &GapTags) {
    progress.phase_verbose(&format_gap_tags_verbose_line(tags));
}

#[cfg(test)]
mod tests {
    use super::{
        format_anchored_retry_verbose_line, format_gap_fill_plan_lines,
        format_gap_fill_result_line, format_patch_characterize_verbose_line,
        format_skip_gap_fill_log, GapFillPlanLog, GapFillResultLog,
    };
    use crate::domain::fill_offset::FillOffsetMode;
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::patch_result::GapPatchSkipReason;

    #[test]
    fn format_gap_fill_plan_lines_shows_mapped_and_search_windows() {
        let lines = format_gap_fill_plan_lines(&GapFillPlanLog {
            scan_a_start_secs: 0.0,
            scan_a_end_secs: 3.0,
            refined_a_start_secs: 0.1,
            refined_a_end_secs: 2.9,
            gap_offset_secs: 61.199,
            fill_offset_mode: FillOffsetMode::Interpolated,
            mapped_b_start_secs: 61.299,
            mapped_b_end_secs: 64.099,
            b_search_start_secs: 50.0,
            b_search_end_secs: 80.0,
            signature_mode_label: "energy",
        });
        assert!(lines
            .iter()
            .any(|l| l.contains("fill offset +61.199s (interpolated)")));
        assert!(lines.iter().any(|l| l.contains("A gap (refined):")));
        assert!(lines.iter().any(|l| l.contains("0:00.100 – 0:02.900")));
        assert!(lines.iter().any(|l| l.contains("B gap (mapped):")));
        assert!(lines.iter().any(|l| l.contains("0:50 – 1:20")));
        assert!(lines.iter().any(|l| l.contains("signature_mode=energy")));
    }

    #[test]
    fn format_gap_fill_result_line_converts_sample_offsets_to_timeline() {
        let line = format_gap_fill_result_line(&GapFillResultLog {
            b_search_start_secs: 50.0,
            sample_rate: 48_000,
            channels: 6,
            fill_start_sample: 48_000 * 6,
            fill_end_sample: 96_000 * 6,
            structure_slide_secs: -0.02,
            waveform_slide_secs: 0.01,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            fit_haystack_secs: 12.0,
            report_pre: 0.31,
            report_post: 1.0,
            confidence: FillConfidence::Marginal,
        });
        assert!(line.contains("B fill source:"));
        assert!(line.contains("structure slide -0.020s"));
        assert!(line.contains("waveform slide +0.010s"));
        assert!(line.contains("0:51.000 – 0:52.000"));
        assert!(line.contains("baseline only (marginal, pre=0.31 post=1.00)"));
    }

    #[test]
    fn format_gap_fill_result_line_shows_boundary_grid_cells() {
        let line = format_gap_fill_result_line(&GapFillResultLog {
            b_search_start_secs: 0.0,
            sample_rate: 48_000,
            channels: 2,
            fill_start_sample: 0,
            fill_end_sample: 96_000,
            structure_slide_secs: 0.0,
            waveform_slide_secs: 0.0,
            fit_used_boundary_grid: true,
            fit_boundary_grid_cells: Some(143),
            fit_haystack_secs: 36.0,
            report_pre: 0.5,
            report_post: 0.5,
            confidence: FillConfidence::High,
        });
        assert!(line.contains("boundary grid (143 cells, haystack 36.0s)"));
    }

    #[test]
    fn skip_gap_fill_log_matches_stdout_gap_number() {
        use crate::domain::format_gap_patch_skip_warn_reason;

        // `1` is the region's report index; the line renders the table's `#2`. No `/total`:
        // the number is an identity, not a progress count.
        assert_eq!(
            format_skip_gap_fill_log(
                1,
                6128.25,
                6360.0,
                &format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BoundaryAlignmentFailed),
            ),
            "gap #2 (1:42:08 – 1:46:00): structure alignment failed"
        );
    }

    #[test]
    fn characterize_verbose_line_separates_report_identity_from_planned_count() {
        // Report gap 4 (0-based) is the 2nd of 3 *planned* regions — the axes diverge because gaps
        // ahead of it were skipped at plan time. The line must show `#5` and `2 of 3`, never one
        // number doing both jobs.
        assert_eq!(
            format_patch_characterize_verbose_line(4, 2, 3, 44.0, 46.5),
            "  gap #5 (2 of 3 planned): A 0:44.000 – 0:46.500"
        );
    }

    #[test]
    fn anchored_retry_verbose_line_is_identity_only() {
        let line = format_anchored_retry_verbose_line("marginal upgrade", 4, 44.0, 46.5);
        assert_eq!(
            line,
            "  anchored marginal upgrade gap #5: A 0:44.000 – 0:46.500"
        );
        // Pass 2 walks a filtered subset, so no `k of m` may appear here.
        assert!(!line.contains(" of "));
        assert_eq!(
            format_anchored_retry_verbose_line("retry", 0, 44.0, 46.5),
            "  anchored retry gap #1: A 0:44.000 – 0:46.500"
        );
    }
}
