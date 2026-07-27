//! Decode / padding quality gates for extracted PCM clips.

use std::borrow::Cow;
use std::time::Duration;

use crate::domain::clip_window::{ClipLabel, ClipWindow};
use crate::domain::mono_pcm_clip::MonoPcmClip;

/// Drop silence padding appended when a tail extract ended before the planned window end.
///
/// Returns [`Cow::Borrowed`] when no sample truncate is needed (including non-padded clips),
/// so callers can avoid a full PCM clone. Allocates only when `decoded_sample_count` marks
/// a shorter decoded region than `samples.len()`.
pub fn truncate_padded_tail(clip: &MonoPcmClip) -> Cow<'_, MonoPcmClip> {
    match clip.decoded_sample_count {
        Some(decoded) if decoded < clip.samples.len() => {
            let mut owned = clip.clone();
            owned.samples.truncate(decoded);
            owned.decoded_sample_count = None;
            Cow::Owned(owned)
        }
        _ => Cow::Borrowed(clip),
    }
}

/// Whether a hold-out extract decoded enough samples for the requested segment length.
pub fn holdout_extract_sufficient(
    clip: &MonoPcmClip,
    segment_length: Duration,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if clip.decode_error_skips > max_decode_skips {
        return false;
    }
    let rate = clip.sample_rate.max(1);
    let expected = ((segment_length.as_secs_f64() * f64::from(rate))
        .floor()
        .max(1.0)) as usize;
    if expected == 0 {
        return false;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) >= (expected as f64) * threshold
}

/// Whether an end-clip extract is too incomplete or corrupt for alignment.
pub fn end_clip_extract_unreliable(
    clip: &MonoPcmClip,
    window: &ClipWindow,
    min_decode_fraction: f64,
    max_decode_skips: u32,
) -> bool {
    if window.label != ClipLabel::End {
        return false;
    }
    if clip.decode_error_skips > max_decode_skips {
        return true;
    }
    let rate = clip.sample_rate.max(1);
    let expected = window.sample_count_at(rate);
    if expected == 0 {
        return true;
    }
    let decoded = clip.effective_decoded_sample_count();
    let threshold = min_decode_fraction.clamp(0.0, 1.0);
    (decoded as f64) < (expected as f64) * threshold
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::time::Duration;

    use super::*;
    use crate::domain::clip_window::{ClipLabel, ClipWindow};
    use crate::domain::mono_pcm_clip::MonoPcmClip;

    #[test]
    fn end_clip_extract_unreliable_when_tail_padding_exceeds_threshold() {
        let window = ClipWindow::new(
            Duration::from_secs(5280),
            Duration::from_secs(6180),
            ClipLabel::End,
        );
        let expected = window.sample_count_at(48_000);
        let decoded = (expected as f64 * 0.94) as usize;
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![0; expected],
            decode_error_skips: 0,
            decoded_sample_count: Some(decoded),
        };
        assert!(end_clip_extract_unreliable(&clip, &window, 0.95, 8));
    }

    #[test]
    fn truncate_padded_tail_removes_synthetic_silence() {
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![1; 100],
            decode_error_skips: 0,
            decoded_sample_count: Some(80),
        };
        let trimmed = truncate_padded_tail(&clip);
        assert!(matches!(trimmed, Cow::Owned(_)));
        assert_eq!(trimmed.samples.len(), 80);
        assert!(trimmed.decoded_sample_count.is_none());
        // Source clip unchanged (borrow path must not mutate in place).
        assert_eq!(clip.samples.len(), 100);
        assert_eq!(clip.decoded_sample_count, Some(80));
    }

    #[test]
    fn truncate_padded_tail_borrows_when_no_padding() {
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![1; 100],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let trimmed = truncate_padded_tail(&clip);
        assert!(matches!(trimmed, Cow::Borrowed(_)));
        assert_eq!(trimmed.samples.len(), 100);
    }

    #[test]
    fn truncate_padded_tail_borrows_when_decoded_covers_buffer() {
        let clip = MonoPcmClip {
            sample_rate: 48_000,
            samples: vec![1; 100],
            decode_error_skips: 0,
            decoded_sample_count: Some(100),
        };
        let trimmed = truncate_padded_tail(&clip);
        assert!(matches!(trimmed, Cow::Borrowed(_)));
        assert_eq!(trimmed.samples.len(), 100);
    }

    #[test]
    fn holdout_extract_sufficient_requires_full_segment_decode() {
        let segment = Duration::from_secs(60);
        let full = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 60],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 60),
        };
        assert!(holdout_extract_sufficient(&full, segment, 0.95, 8));

        let short = MonoPcmClip {
            sample_rate: 11_025,
            samples: vec![0_i16; 11_025 * 30],
            decode_error_skips: 0,
            decoded_sample_count: Some(11_025 * 30),
        };
        assert!(!holdout_extract_sufficient(&short, segment, 0.95, 8));
    }
}
