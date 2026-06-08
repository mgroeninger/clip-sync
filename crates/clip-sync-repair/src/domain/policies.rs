use clip_sync::MonoPcmClip;

/// Returns true if the clip's RMS energy is below `silence_peak_fraction` of its peak amplitude.
///
/// Leading silence is preserved by `ScanGaps` window placement, so a fully-zero clip
/// (e.g., padding at end of file) is considered silent.
pub fn is_silent(clip: &MonoPcmClip, silence_peak_fraction: f32) -> bool {
    if clip.samples.is_empty() {
        return true;
    }

    let peak = clip
        .samples
        .iter()
        .map(|s| s.unsigned_abs())
        .max()
        .unwrap_or(0) as f32;

    if peak == 0.0 {
        return true;
    }

    let threshold = peak * silence_peak_fraction;
    let rms = rms_i16(&clip.samples);
    rms < threshold
}

fn rms_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|s| {
            let v = f64::from(*s);
            v * v
        })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(samples: Vec<i16>) -> MonoPcmClip {
        MonoPcmClip {
            sample_rate: 44_100,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    #[test]
    fn empty_clip_is_silent() {
        assert!(is_silent(&clip(vec![]), 0.01));
    }

    #[test]
    fn all_zeros_is_silent() {
        assert!(is_silent(&clip(vec![0; 1000]), 0.01));
    }

    #[test]
    fn loud_sine_is_not_silent() {
        let samples: Vec<i16> = (0..1000)
            .map(|i| (f32::sin(i as f32 * 0.1) * 10_000.0) as i16)
            .collect();
        assert!(!is_silent(&clip(samples), 0.01));
    }

    #[test]
    fn single_spike_in_sea_of_zeros_is_silent() {
        // Peak = 100, threshold = 100 * 0.01 = 1.0.
        // 1 spike in 11025 zeros: RMS = sqrt(10000/11025) ≈ 0.95 < 1.0 → silent.
        let mut samples = vec![0i16; 11_025];
        samples[0] = 100;
        assert!(is_silent(&clip(samples), 0.01));
    }
}
