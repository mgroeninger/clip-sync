//! Per-gap B timeline offset from alignment clip pair (recommended vs interpolated drift).

use clip_sync::{AlignmentReport, ClipLabelReport};

/// How patch maps each gap on A to B's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillOffsetMode {
    /// Use `recommended_offset_secs` for every gap (scan + patch).
    #[default]
    Recommended,
    /// Linearly interpolate between start-clip and end-clip offsets by position on A.
    Interpolated,
}

/// Minimum |end − start| clip offset drift before interpolation differs from a single offset.
const MIN_DRIFT_FOR_INTERPOLATION_SECS: f64 = 0.05;

/// Offset (seconds) to add to A's timeline to reach B: `b_secs = a_secs + offset`.
pub fn fill_offset_secs(
    alignment: &AlignmentReport,
    mode: FillOffsetMode,
    gap_time_on_a_secs: f64,
) -> Option<f64> {
    match mode {
        FillOffsetMode::Recommended => alignment.recommended_offset_secs,
        FillOffsetMode::Interpolated => interpolated_offset_secs(alignment, gap_time_on_a_secs)
            .or(alignment.recommended_offset_secs),
    }
}

fn interpolated_offset_secs(
    alignment: &AlignmentReport,
    gap_time_on_a_secs: f64,
) -> Option<f64> {
    let (start_offset, start_anchor) = clip_offset_and_anchor(alignment, ClipLabelReport::Start)?;
    let (end_offset, end_anchor) = clip_offset_and_anchor(alignment, ClipLabelReport::End)?;

    let drift = end_offset - start_offset;
    if drift.abs() < MIN_DRIFT_FOR_INTERPOLATION_SECS {
        return Some(start_offset);
    }

    let span = end_anchor - start_anchor;
    if span.abs() < f64::EPSILON {
        return Some(start_offset);
    }

    let t = gap_time_on_a_secs.clamp(start_anchor.min(end_anchor), start_anchor.max(end_anchor));
    let fraction = (t - start_anchor) / span;
    Some(start_offset + drift * fraction)
}

fn clip_offset_and_anchor(
    alignment: &AlignmentReport,
    label: ClipLabelReport,
) -> Option<(f64, f64)> {
    let clip = alignment
        .clips
        .iter()
        .find(|clip| clip.label == label && clip.aligned)?;
    let offset = clip.offset_secs?;
    let anchor = (clip.window_start_secs + clip.window_end_secs) / 2.0;
    Some((offset, anchor))
}

#[cfg(test)]
mod tests {
    use clip_sync::{AlignmentReport, AlignmentResult, ClipLabel, ClipLabelReport, ClipMatch};

    use super::*;

    fn two_clip_alignment(start_offset: f64, end_offset: f64) -> AlignmentReport {
        AlignmentReport::from(&AlignmentResult {
            clips: vec![
                ClipMatch {
                    label: ClipLabel::Start,
                    window_start_secs: 0.0,
                    window_end_secs: 900.0,
                    aligned: true,
                    offset_secs: Some(start_offset),
                    confidence: 0.95,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                },
                ClipMatch {
                    label: ClipLabel::End,
                    window_start_secs: 6647.0,
                    window_end_secs: 7547.0,
                    aligned: true,
                    offset_secs: Some(end_offset),
                    confidence: 0.95,
                    video_a_decode_skips: 0,
                    video_b_decode_skips: 0,
                    repetition: None,
                    video_b_window_start_secs: None,
                    video_b_window_end_secs: None,
                },
            ],
            start_aligned: true,
            end_aligned: Some(true),
            recommended_offset_secs: Some((start_offset + end_offset) / 2.0),
            offsets_consistent: false,
            offset_drift_secs: Some(end_offset - start_offset),
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
            offset_ambiguous_mod_secs: None,
            alignment_mode_used: None,
            query_localization: None,
            end_clip_anchor: None,
        })
    }

    #[test]
    fn recommended_mode_uses_headline_offset() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let offset = fill_offset_secs(
            &alignment,
            FillOffsetMode::Recommended,
            1610.0,
        )
        .unwrap();
        assert!((offset - (-7.0)).abs() < 0.01);
    }

    #[test]
    fn interpolated_offset_at_start_anchor_matches_start_clip() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let start_anchor = 450.0;
        let offset = fill_offset_secs(
            &alignment,
            FillOffsetMode::Interpolated,
            start_anchor,
        )
        .unwrap();
        assert!((offset - (-7.326)).abs() < 0.001);
    }

    #[test]
    fn interpolated_offset_at_mid_timeline() {
        let alignment = two_clip_alignment(-7.326, -6.674);
        let start_anchor = 450.0;
        let end_anchor = 7097.0;
        let mid = (start_anchor + end_anchor) / 2.0;
        let offset = fill_offset_secs(&alignment, FillOffsetMode::Interpolated, mid).unwrap();
        let expected = -7.326 + 0.652 * 0.5;
        assert!((offset - expected).abs() < 0.01);
    }

    #[test]
    fn interpolated_falls_back_when_only_start_clip() {
        let mut alignment = two_clip_alignment(-3.0, -2.0);
        alignment.clips.retain(|c| c.label == ClipLabelReport::Start);
        alignment.recommended_offset_secs = Some(-3.0);
        let offset = fill_offset_secs(&alignment, FillOffsetMode::Interpolated, 100.0).unwrap();
        assert!((offset - (-3.0)).abs() < f64::EPSILON);
    }
}
