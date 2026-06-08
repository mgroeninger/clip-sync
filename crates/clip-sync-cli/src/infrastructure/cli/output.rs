use clip_sync::{AppError, AlignmentResult, ClipLabel, ClipMatch};

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
    println!("Alignment report");
    println!("  Start clip aligned: {}", yes_no(result.start_aligned));

    if let Some(end_aligned) = result.end_aligned {
        println!("  End clip aligned: {}", yes_no(end_aligned));
    }

    for clip in &result.clips {
        println!("  {}", format_clip_line(clip, show_diagnostics));
    }

    match result.recommended_offset_secs {
        Some(offset) => println!(
            "  Recommended offset: {:+.3}s ({})",
            offset,
            if result.offsets_consistent {
                "clip offsets agree"
            } else if result.start_aligned {
                "clip offsets disagree; using start clip"
            } else {
                "clip offsets disagree"
            }
        ),
        None => println!("  Recommended offset: none"),
    }

    if let Some(drift) = result.offset_drift_secs {
        if !result.offsets_consistent {
            println!("  Offset drift (end − start): {:+.3}s", drift);
        }
    }

    if let Some(overlap) = result.start_overlap {
        println!("  Overlap (from start clip):");
        println!(
            "    On video A:  {}",
            format_window(overlap.video_a_start_secs, overlap.video_a_end_secs)
        );
        println!(
            "    On video B:  {}",
            format_window(overlap.video_b_start_secs, overlap.video_b_end_secs)
        );
        println!(
            "    Length:      {}",
            format_timestamp(overlap.shared_length_secs)
        );
    }

    if let Some(refine) = &result.high_rate_refinement {
        if refine.applied {
            println!(
                "  High-rate refinement: {:+.3}s adjustment (peak {:.2})",
                refine.adjustment_secs, refine.correlation_peak
            );
        } else if show_diagnostics {
            let reason = refine
                .skip_reason
                .as_deref()
                .unwrap_or("not applied");
            println!("  High-rate refinement: skipped ({reason})");
        }
    }
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
