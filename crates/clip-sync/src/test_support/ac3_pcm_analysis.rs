//! PCM metrics for AC-3 decode characterization (oxideav vs ffmpeg reference).

/// Count interleaved S16 samples with `abs(sample) >= i16::MAX` (full-scale railing).
pub fn railed_sample_count(samples: &[i16]) -> usize {
    samples.iter().filter(|sample| sample.abs() >= i16::MAX).count()
}

/// Count normalized f32 samples at full scale (`|s| >= 1.0`).
pub fn railed_sample_count_f32(samples: &[f32]) -> usize {
    samples.iter().filter(|sample| sample.abs() >= 1.0).count()
}

/// Peak absolute sample value in interleaved S16 PCM.
pub fn peak_abs(samples: &[i16]) -> i16 {
    samples.iter().map(|sample| sample.abs()).max().unwrap_or(0)
}

/// Peak absolute sample value in normalized f32 PCM.
pub fn peak_abs_f32(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample.abs()).fold(0.0f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn railed_sample_count_detects_full_scale_only() {
        assert_eq!(railed_sample_count(&[0, 32_766, -32_766, i16::MAX, -i16::MAX]), 2);
    }
}
