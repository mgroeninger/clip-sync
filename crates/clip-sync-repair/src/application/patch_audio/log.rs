use clip_sync::{format_time_range_verbose, ProgressReporter};

use crate::domain::fill_offset::FillOffsetMode;
use crate::domain::gap_fill::FillRegion;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::gap_tags::{format_gap_tags_verbose_line, GapTags};
use crate::domain::patch_result::{
    format_gap_fill_marginal_verbose_line, format_gap_fill_marginal_warn_reason,
    format_gap_fill_skip_verbose_line, format_gap_patch_skip_warn_reason, GapPatchSkipReason,
};
use crate::domain::Gap;

fn fill_offset_mode_label(mode: FillOffsetMode) -> &'static str {
    match mode {
        FillOffsetMode::Recommended => "recommended",
        FillOffsetMode::Interpolated => "interpolated",
        FillOffsetMode::Anchored => "anchored",
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

/// Human-readable skip line for stderr (`tracing::warn`) matching the stdout gap table.
///
/// `gap_index` is the region's [`FillRegion::gap_index`] — the gap's position in the report.
/// An out-of-range index drops the `N/M` prefix rather than mislabelling the gap.
pub(crate) fn format_skip_gap_fill_log(
    gaps: &[Gap],
    gap_index: usize,
    a_start_secs: f64,
    a_end_secs: f64,
    reason: &str,
) -> String {
    let total = gaps.len();
    let range = format_time_range_verbose(a_start_secs, a_end_secs);
    if gap_index < total {
        format!(
            "gap {index}/{total} ({range}): {reason}",
            index = gap_index + 1
        )
    } else {
        format!("gap ({range}): {reason}")
    }
}

pub(super) fn log_skip_gap_fill(
    progress: &dyn ProgressReporter,
    gaps: &[Gap],
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
                gaps,
                region.gap_index,
                region.a_start_secs,
                region.a_end_secs,
                &format_gap_patch_skip_warn_reason(reason),
            )
        );
    }
}

pub(super) struct MarginalGapFillLog<'a> {
    pub(super) gaps: &'a [Gap],
    /// Index into `gaps`; see [`FillRegion::gap_index`].
    pub(super) gap_index: usize,
    pub(super) a_start_secs: f64,
    pub(super) a_end_secs: f64,
    pub(super) pre: f64,
    pub(super) post: f64,
    pub(super) min: f32,
    pub(super) anchor_seam: bool,
}

pub(super) fn log_marginal_gap_fill(progress: &dyn ProgressReporter, log: &MarginalGapFillLog<'_>) {
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
                log.gaps,
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
        format_gap_fill_plan_lines, format_gap_fill_result_line, format_skip_gap_fill_log,
        GapFillPlanLog, GapFillResultLog,
    };
    use crate::domain::fill_offset::FillOffsetMode;
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::patch_result::GapPatchSkipReason;
    use crate::domain::Gap;

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

        let gaps = vec![
            Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 8.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
            Gap {
                video_a_start_secs: 6128.25,
                video_a_end_secs: 6360.0,
                video_b_start_secs: Some(0.0),
                video_b_end_secs: Some(1.0),
                b_has_energy: true,
            },
        ];

        assert_eq!(
            format_skip_gap_fill_log(
                &gaps,
                1,
                6128.25,
                6360.0,
                &format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BoundaryAlignmentFailed),
            ),
            "gap 2/2 (1:42:08 – 1:46:00): structure alignment failed"
        );
    }
}
