use clip_sync::{AppError, AlignmentResult, ClipLabel, ClipMatch, RepetitionFinding};

use crate::infrastructure::config::{OutputConfig, OutputFormat};

pub fn print_success(output: &OutputConfig, result: &AlignmentResult) -> Result<(), AppError> {
    match output.format {
        OutputFormat::Human => print_human(output.show_diagnostics, result),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }

    Ok(())
}

fn print_human(show_diagnostics: bool, result: &AlignmentResult) {
    print!("{}", format_human_output(show_diagnostics, result));
}

pub fn format_human_output(show_diagnostics: bool, result: &AlignmentResult) -> String {
    let mut out = String::new();
    out.push_str("Alignment report\n");
    out.push_str(&format!("  Start clip aligned: {}\n", yes_no(result.start_aligned)));

    if let Some(end_aligned) = result.end_aligned {
        out.push_str(&format!("  End clip aligned: {}\n", yes_no(end_aligned)));
    }

    for clip in &result.clips {
        out.push_str(&format!("  {}\n", format_clip_line(clip, show_diagnostics)));
        for line in format_repetition_lines(clip, show_diagnostics) {
            out.push_str(&format!("    {line}\n"));
        }
    }

    match result.recommended_offset_secs {
        Some(offset) => out.push_str(&format!(
            "  Recommended offset: {:+.3}s ({})\n",
            offset,
            if result.offsets_consistent {
                "clip offsets agree"
            } else if result.start_aligned {
                "clip offsets disagree; using start clip"
            } else {
                "clip offsets disagree"
            }
        )),
        None => out.push_str("  Recommended offset: none\n"),
    }

    if let Some(drift) = result.offset_drift_secs {
        if !result.offsets_consistent {
            out.push_str(&format!("  Offset drift (end − start): {:+.3}s\n", drift));
        }
    }

    if let Some(overlap) = result.start_overlap {
        out.push_str("  Overlap (from start clip):\n");
        out.push_str(&format!(
            "    On video A:  {}\n",
            format_window(overlap.video_a_start_secs, overlap.video_a_end_secs)
        ));
        out.push_str(&format!(
            "    On video B:  {}\n",
            format_window(overlap.video_b_start_secs, overlap.video_b_end_secs)
        ));
        out.push_str(&format!(
            "    Length:      {}\n",
            format_timestamp(overlap.shared_length_secs)
        ));
    }

    if let Some(refine) = &result.high_rate_refinement {
        if refine.applied {
            out.push_str(&format!(
                "  High-rate refinement: {:+.3}s adjustment (peak {:.2})\n",
                refine.adjustment_secs, refine.correlation_peak
            ));
        } else if show_diagnostics {
            let reason = refine.skip_reason.as_deref().unwrap_or("not applied");
            out.push_str(&format!("  High-rate refinement: skipped ({reason})\n"));
        }
    }

    out
}

fn format_clip_line(clip: &ClipMatch, show_diagnostics: bool) -> String {
    let label = clip_label_name(clip.label);
    let window = format_window(clip.window_start_secs, clip.window_end_secs);

    let mut line = if clip.aligned {
        format!(
            "{label} clip {window}: aligned, offset {:+.3}s (confidence {:.2})",
            clip.offset_secs.unwrap_or(0.0),
            clip.confidence
        )
    } else {
        format!(
            "{label} clip {window}: not aligned (confidence {:.2})",
            clip.confidence
        )
    };

    if show_diagnostics && (clip.video_a_decode_skips > 0 || clip.video_b_decode_skips > 0) {
        line.push_str(&format!(
            " [decode skips: A={}, B={}]",
            clip.video_a_decode_skips, clip.video_b_decode_skips
        ));
    }

    line
}

fn format_repetition_lines(clip: &ClipMatch, show_diagnostics: bool) -> Vec<String> {
    let Some(rep) = &clip.repetition else {
        return vec![];
    };
    let mut lines = Vec::new();
    for (label, finding) in [("video A", &rep.a), ("video B", &rep.b)] {
        match finding {
            Some(f) => lines.push(format_repetition_finding(label, f)),
            None if show_diagnostics => {
                lines.push(format!("{label}: no internal repeat detected"));
            }
            None => {}
        }
    }
    lines
}

fn format_repetition_finding(label: &str, finding: &RepetitionFinding) -> String {
    format!(
        "{label}: internal repeat ~{:.1}s (confidence {:.2})",
        finding.lag_secs, finding.confidence
    )
}

fn format_window(start_secs: f64, end_secs: f64) -> String {
    format!(
        "[{}–{}]",
        format_timestamp(start_secs),
        format_timestamp(end_secs)
    )
}

fn format_timestamp(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn clip_label_name(label: ClipLabel) -> &'static str {
    match label {
        ClipLabel::Start => "Start",
        ClipLabel::Interior => "Interior",
        ClipLabel::End => "End",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
