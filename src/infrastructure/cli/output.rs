use crate::application::config::{OutputConfig, OutputFormat};
use crate::application::error::AppError;
use crate::domain::AlignmentResult;

pub fn print_success(output: &OutputConfig, result: &AlignmentResult) -> Result<(), AppError> {
    match output.format {
        OutputFormat::Human => {
            println!(
                "Offset: {:+.3}s (confidence: {:.2})",
                result.offset_secs, result.confidence
            );
        }
        OutputFormat::Json => {
            let payload = JsonAlignmentResult::from(result);
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct JsonAlignmentResult<'a> {
    offset_secs: f64,
    confidence: f32,
    per_clip_offsets: &'a [f64],
}

impl<'a> From<&'a AlignmentResult> for JsonAlignmentResult<'a> {
    fn from(result: &'a AlignmentResult) -> Self {
        Self {
            offset_secs: result.offset_secs,
            confidence: result.confidence,
            per_clip_offsets: &result.per_clip_offsets,
        }
    }
}
