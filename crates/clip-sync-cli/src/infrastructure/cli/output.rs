use clip_sync::{
    format_high_rate_refinement_lines, format_offset_verification_lines,
    format_periodic_ambiguity_line, format_query_localization_lines, format_timestamp,
    AlignmentReport, AlignmentResult, AppError, ClipLabelReport, ClipMatchReport,
    RepetitionFindingReport,
};

use crate::infrastructure::config::{OutputConfig, OutputFormat};

/// JSON report for stdout (`--format json`). Golden-tested in `clip-sync-cli/tests/cli_output.rs`.
pub fn format_json_output(result: &AlignmentResult) -> String {
    serde_json::to_string_pretty(&AlignmentReport::from(result)).expect("serialize alignment report")
}

pub fn print_success(output: &OutputConfig, result: &AlignmentResult) -> Result<(), AppError> {
    match output.format {
        OutputFormat::Human => print_human(output.show_diagnostics, result),
        OutputFormat::Json => println!("{}", format_json_output(result)),
    }

    Ok(())
}

fn print_human(show_diagnostics: bool, result: &AlignmentResult) {
    print!("{}", format_human_output(show_diagnostics, result));
}

pub fn format_human_output(show_diagnostics: bool, domain_result: &AlignmentResult) -> String {
    let headline_confidence = domain_result
        .start_clip()
        .or_else(|| domain_result.clips.first())
        .map(|clip| format!("{:.2}", clip.confidence))
        .unwrap_or_else(|| "n/a".into());

    let result = AlignmentReport::from(domain_result);
    let mut out = String::new();
    let query_mode = result.query_localization.is_some();

    if query_mode {
        if let Some(loc) = &result.query_localization {
            for line in format_query_localization_lines(loc, show_diagnostics) {
                out.push_str(&line);
                out.push('\n');
            }
        }
    } else {
        let offset = result
            .recommended_offset_secs
            .map(|o| format!("{o:+.3}s"))
            .unwrap_or_else(|| "n/a".into());
        let confidence = headline_confidence;
        out.push_str(&format!("Alignment: offset {offset}  confidence {confidence}\n"));
    }

    let show_per_clip_offsets = !query_mode && result.clips.len() > 1;
    let show_clip_window_lines = show_diagnostics
        || result.clips.iter().any(|clip| !clip.aligned);

    if show_per_clip_offsets {
        for clip in &result.clips {
            out.push_str(&format_per_clip_offset_line(clip));
            for line in format_repetition_lines(clip, show_diagnostics) {
                out.push_str(&format!("    {line}\n"));
            }
        }
    } else if show_clip_window_lines {
        for clip in &result.clips {
            out.push_str(&format!(
                "  {}\n",
                format_clip_window_line(clip, show_diagnostics)
            ));
            for line in format_repetition_lines(clip, show_diagnostics) {
                out.push_str(&format!("    {line}\n"));
            }
        }
    } else if !query_mode {
        for clip in &result.clips {
            for line in format_repetition_lines(clip, show_diagnostics) {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }

    if !query_mode {
        if let Some(drift) = result.offset_drift_secs {
            if !result.offsets_consistent {
                out.push_str(&format!("Drift:     end − start = {drift:+.3}s\n"));
                if result.recommended_offset_secs.is_some() {
                    out.push_str("           using start-clip offset (clip offsets disagree)\n");
                }
            }
        }
    }

    if !query_mode {
        if let Some(overlap) = result.start_overlap {
            out.push_str(&format!(
                "Overlap:   A {}   B {}   ({} shared)\n",
                format_window(overlap.video_a_start_secs, overlap.video_a_end_secs),
                format_window(overlap.video_b_start_secs, overlap.video_b_end_secs),
                format_timestamp(overlap.shared_length_secs),
            ));
        }
    }

    if let Some(refine) = &result.high_rate_refinement {
        for line in format_high_rate_refinement_lines(refine, show_diagnostics) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if let Some(period) = result.offset_ambiguous_mod_secs {
        out.push_str(&format_periodic_ambiguity_line(period));
        out.push('\n');
    }

    if let Some(verify) = &result.offset_verification {
        for line in format_offset_verification_lines(verify, show_diagnostics) {
            out.push_str(&line);
            out.push('\n');
        }
    }

    out
}

fn format_per_clip_offset_line(clip: &ClipMatchReport) -> String {
    let label = clip_label_name(clip.label);
    if clip.aligned {
        if let Some(offset) = clip.offset_secs {
            return format!(
                "  {label} clip: {offset:+.3}s  (confidence {:.2})\n",
                clip.confidence
            );
        }
    }
    format!(
        "  {label} clip: not aligned (confidence {:.2})\n",
        clip.confidence
    )
}

fn format_clip_window_line(clip: &ClipMatchReport, show_diagnostics: bool) -> String {
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

fn format_repetition_lines(clip: &ClipMatchReport, show_diagnostics: bool) -> Vec<String> {
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

fn format_repetition_finding(label: &str, finding: &RepetitionFindingReport) -> String {
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

fn clip_label_name(label: ClipLabelReport) -> &'static str {
    match label {
        ClipLabelReport::Start => "Start",
        ClipLabelReport::Interior => "Interior",
        ClipLabelReport::End => "End",
    }
}
