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

/// RMS of interleaved (multi-channel) i16 samples.
pub fn rms_interleaved(samples: &[i16]) -> f32 {
    rms_i16(samples)
}

/// Compute a gain factor to match `b_segment_rms` to `a_border_rms`.
///
/// The gain is clamped to ±`max_gain_db` dB. Returns `1.0` when either RMS is zero.
pub fn compute_fill_gain(a_border_rms: f32, b_segment_rms: f32, max_gain_db: f64) -> f32 {
    if a_border_rms == 0.0 || b_segment_rms == 0.0 {
        return 1.0;
    }
    let gain = a_border_rms / b_segment_rms;
    let max_gain = 10f32.powf((max_gain_db / 20.0) as f32);
    let min_gain = 1.0 / max_gain;
    gain.clamp(min_gain, max_gain)
}

/// Equal-power crossfade: blend `fill` into `into` at both seams.
///
/// The effective crossfade length is `crossfade_frames.min(total_frames / 2)`.
/// - Fade-in (first `cf` frames): a_w = cos(t*π/2), b_w = sin(t*π/2)
/// - Middle: pure fill
/// - Fade-out (last `cf` frames): a_w = sin(t*π/2), b_w = cos(t*π/2)
///
/// Samples are written into `into` — `into` contains A's original samples and is replaced.
pub fn apply_crossfade(into: &mut [i16], fill: &[i16], channels: usize, crossfade_frames: usize) {
    let channels = channels.max(1);
    let total_frames = into.len() / channels;
    let cf = crossfade_frames.min(total_frames / 2);

    for frame in 0..total_frames {
        if cf == 0 || (frame >= cf && frame < total_frames - cf) {
            // Middle: pure fill
            for ch in 0..channels {
                let idx = frame * channels + ch;
                if idx < fill.len() {
                    into[idx] = fill[idx];
                }
            }
        } else if frame < cf {
            // Fade-in: blend from A into fill
            let t = frame as f32 / cf as f32;
            let a_w = (t * std::f32::consts::FRAC_PI_2).cos();
            let b_w = (t * std::f32::consts::FRAC_PI_2).sin();
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let a_val = into[idx] as f32;
                let b_val = if idx < fill.len() { fill[idx] as f32 } else { 0.0 };
                into[idx] = (a_w * a_val + b_w * b_val)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
        } else {
            // Fade-out: blend from fill back into A
            let t = (frame - (total_frames - cf)) as f32 / cf as f32;
            let a_w = (t * std::f32::consts::FRAC_PI_2).sin();
            let b_w = (t * std::f32::consts::FRAC_PI_2).cos();
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let a_val = into[idx] as f32;
                let b_val = if idx < fill.len() { fill[idx] as f32 } else { 0.0 };
                into[idx] = (a_w * a_val + b_w * b_val)
                    .round()
                    .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
        }
    }
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

    #[test]
    fn rms_interleaved_of_constant() {
        let samples = vec![1000i16; 100];
        let result = rms_interleaved(&samples);
        assert!((result - 1000.0).abs() < 1.0, "rms of constant 1000 should be ~1000, got {result}");
    }

    #[test]
    fn compute_fill_gain_clamps_to_max_db() {
        // a=1000, b=1 => raw gain=1000; max_db=12 => max_gain=10^(12/20)≈3.981
        let gain = compute_fill_gain(1000.0, 1.0, 12.0);
        let expected_max = 10f32.powf(12.0 / 20.0);
        assert!((gain - expected_max).abs() < 0.001, "gain should be clamped to {expected_max}, got {gain}");
    }

    #[test]
    fn compute_fill_gain_unity_when_rms_zero() {
        assert_eq!(compute_fill_gain(0.0, 500.0, 12.0), 1.0);
        assert_eq!(compute_fill_gain(500.0, 0.0, 12.0), 1.0);
    }

    #[test]
    fn apply_crossfade_middle_is_pure_fill() {
        // 10 mono frames, fill = all 1000, A = all 0, cf=2
        let fill = vec![1000i16; 10];
        let mut into = vec![0i16; 10];
        apply_crossfade(&mut into, &fill, 1, 2);
        // Middle frames [2..8) should be pure fill = 1000
        for i in 2..8 {
            assert_eq!(into[i], 1000, "frame {i} should be 1000 (pure fill)");
        }
    }

    #[test]
    fn apply_crossfade_is_continuous() {
        // A=0, B=1000, cf=4, n=10 mono frames
        let fill = vec![1000i16; 10];
        let mut into = vec![0i16; 10];
        apply_crossfade(&mut into, &fill, 1, 4);
        for i in 1..into.len() {
            let diff = (into[i] as i32 - into[i - 1] as i32).abs();
            assert!(diff <= 500, "jump of {diff} between frame {} and {} (values {} {})", i-1, i, into[i-1], into[i]);
        }
    }
}
