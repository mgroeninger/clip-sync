use clip_sync::ClipLabel;
use serde::Serialize;

use crate::application::error::RepairError;
use crate::application::ports::GapReporter;
use crate::domain::{
    CompatibilityVerdict, GapFillSkipReason, GapPatchSkipReason, GapPatchStatus, GapReport,
    PatchSummary,
};
use crate::infrastructure::config::OutputFormat;

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

fn format_human(report: &GapReport) -> String {
    let mut out = String::new();

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

    if let Some(drift) = report.alignment.offset_drift_secs {
        if !report.alignment.offsets_consistent {
            out.push_str(&format!("Drift:     end − start = {drift:+.3}s\n"));
            if report.alignment.recommended_offset_secs.is_some() {
                out.push_str("           using start-clip offset for fill (clip offsets disagree)\n");
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

    out.push('\n');

    if report.gaps.is_empty() {
        out.push_str("No gaps detected in video A.\n");
        return out;
    }

    out.push_str(&format!(
        "Gaps detected in video A ({} total, {} fillable):\n",
        report.gaps.len(),
        report.fillable_count()
    ));
    if report.alignment.recommended_offset_secs.is_none() {
        out.push_str("  B timeline mapping skipped (no alignment offset).\n");
    }
    out.push('\n');

    for (i, gap) in report.gaps.iter().enumerate() {
        let fillable = if gap.is_fillable() {
            "fillable"
        } else {
            "unfillable"
        };
        out.push_str(&format!(
            "  #{:<3} [{:>8.2}s – {:>8.2}s]  ({:.1}s)  {}\n",
            i + 1,
            gap.video_a_start_secs,
            gap.video_a_end_secs,
            gap.duration_secs(),
            fillable,
        ));
    }

    out
}

fn clip_label_name(label: ClipLabel) -> &'static str {
    match label {
        ClipLabel::Start => "Start",
        ClipLabel::Interior => "Interior",
        ClipLabel::End => "End",
    }
}

fn print_human(report: &GapReport) -> Result<(), RepairError> {
    print!("{}", format_human(report));
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
    format: OutputFormat,
) -> Result<(), RepairError> {
    match format {
        OutputFormat::Human => {
            print!("{}", format_human(report));
            if let Some(summary) = patch {
                print!("{}", format_patch_summary(summary));
            }
            Ok(())
        }
        OutputFormat::Json => print_json_with_patch(report, patch),
    }
}

fn print_json_with_patch(
    report: &GapReport,
    patch: Option<&PatchSummary>,
) -> Result<(), RepairError> {
    let payload = RepairJsonOutput { scan: report, patch };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| RepairError::Config(format!("JSON serialization failed: {e}")))?;
    println!("{json}");
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
                structure_trusted,
            } => {
                if *structure_trusted {
                    format!(
                        "patched  (struct pre={pre_correlation:.2} post={post_correlation:.2} slide={align_adjustment_secs:+.3}s)"
                    )
                } else {
                    format!(
                        "patched  (pre={pre_correlation:.2} post={post_correlation:.2} slide={align_adjustment_secs:+.3}s)"
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
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{format_patch_summary, RepairJsonOutput};
    use crate::domain::gap::{Gap, GapReport};
    use crate::domain::{CompatibilityVerdict, TrackCompatibility};
    use clip_sync::{AlignmentResult, ClipLabel, ClipMatch, TimelineOverlap};

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
            overlap: Some(overlap),
            alignment: AlignmentResult {
                clips: vec![ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: true,
                    offset_secs: Some(12.5),
                    confidence: 0.88,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                }],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: Some(12.5),
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: Some(overlap),
                high_rate_refinement: None,
            },
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
        }
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
                structure_trusted: false,
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
                    structure_trusted: true,
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
            alignment: AlignmentResult {
                clips: vec![ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: false,
                    offset_secs: None,
                    confidence: 0.2,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                }],
                start_aligned: false,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                high_rate_refinement: None,
            },
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
        }
    }

    #[test]
    fn human_report_shows_drift_when_clips_disagree() {
        let mut report = minimal_report();
        report.alignment.clips = vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 900.0,
                aligned: true,
                offset_secs: Some(-10.956),
                confidence: 0.94,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
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
            },
        ];
        report.alignment.end_aligned = Some(true);
        report.alignment.offsets_consistent = false;
        report.alignment.offset_drift_secs = Some(-0.244);
        report.alignment.recommended_offset_secs = Some(-10.956);

        let text = super::format_human(&report);
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
        let text = super::format_human(&report);
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
        let text = super::format_human(&report);
        assert!(text.contains("MISMATCH"));
        assert!(text.contains("WARNING"));
    }

    #[test]
    fn human_failed_alignment_notes_b_mapping_skipped() {
        let text = super::format_human(&failed_alignment_report());
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
