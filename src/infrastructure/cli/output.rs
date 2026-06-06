use crate::application::config::{OutputConfig, OutputFormat};
use crate::application::error::AppError;
use crate::domain::{AlignmentResult, ClipLabel, ClipMatch};

pub fn print_success(output: &OutputConfig, result: &AlignmentResult) -> Result<(), AppError> {
    match output.format {
        OutputFormat::Human => print_human(result),
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }

    Ok(())
}

fn print_human(result: &AlignmentResult) {
    println!("Alignment report");
    println!("  Start clip aligned: {}", yes_no(result.start_aligned));

    if let Some(end_aligned) = result.end_aligned {
        println!("  End clip aligned: {}", yes_no(end_aligned));
    }

    for clip in &result.clips {
        println!("  {}", format_clip_line(clip));
    }

    match result.recommended_offset_secs {
        Some(offset) => println!(
            "  Recommended offset: {:+.3}s ({})",
            offset,
            if result.offsets_consistent {
                "clip offsets agree"
            } else {
                "clip offsets disagree"
            }
        ),
        None => println!("  Recommended offset: none"),
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
}

fn format_clip_line(clip: &ClipMatch) -> String {
    let label = clip_label_name(clip.label);
    let window = format_window(clip.window_start_secs, clip.window_end_secs);

    if clip.aligned {
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
    }
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
