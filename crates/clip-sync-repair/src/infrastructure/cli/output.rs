use crate::application::error::RepairError;
use crate::application::ports::GapReporter;
use crate::domain::{Gap, GapReport};

pub struct StdoutGapReporter {
    pub format: super::super::config::OutputFormat,
}

impl GapReporter for StdoutGapReporter {
    fn report(&self, report: &GapReport) -> Result<(), RepairError> {
        use super::super::config::OutputFormat;
        match self.format {
            OutputFormat::Human => print_human(report),
            OutputFormat::Json => print_json(report),
        }
    }
}

fn print_human(report: &GapReport) -> Result<(), RepairError> {
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

    println!("Alignment: offset {offset}  confidence {confidence}");
    println!();

    if report.gaps.is_empty() {
        println!("No gaps detected in video A.");
        return Ok(());
    }

    println!(
        "Gaps detected in video A ({} total, {} fillable):",
        report.gaps.len(),
        report.fillable_count()
    );
    println!();

    for (i, gap) in report.gaps.iter().enumerate() {
        let fillable = if gap.b_has_energy { "fillable" } else { "unfillable" };
        println!(
            "  #{:<3} [{:>8.2}s – {:>8.2}s]  ({:.1}s)  {}",
            i + 1,
            gap.video_a_start_secs,
            gap.video_a_end_secs,
            gap.duration_secs(),
            fillable,
        );
    }

    Ok(())
}

fn print_json(report: &GapReport) -> Result<(), RepairError> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| RepairError::Config(format!("JSON serialization failed: {e}")))?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::gap::{Gap, GapReport};
    use clip_sync::{AlignmentResult, ClipLabel, ClipMatch};

    fn minimal_report() -> GapReport {
        GapReport {
            video_a: PathBuf::from("a.mp4"),
            video_b: PathBuf::from("b.mp4"),
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
                start_overlap: None,
                high_rate_refinement: None,
            },
            gaps: vec![Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 60.0,
                video_b_start_secs: 12.5,
                video_b_end_secs: 72.5,
                b_has_energy: true,
            }],
            scan_window_secs: 60,
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
    }

    #[test]
    fn gap_duration_matches_window() {
        let gap = Gap {
            video_a_start_secs: 100.0,
            video_a_end_secs: 160.0,
            video_b_start_secs: 112.5,
            video_b_end_secs: 172.5,
            b_has_energy: false,
        };
        assert!((gap.duration_secs() - 60.0).abs() < 0.001);
        assert!(!gap.is_fillable());
    }
}
