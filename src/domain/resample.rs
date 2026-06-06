use crate::domain::MonoPcmClip;

/// Linearly resample mono PCM to `target_rate`. Returns the input unchanged when rates match.
pub fn resample_mono_pcm(clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip {
    if clip.sample_rate == target_rate || clip.samples.is_empty() {
        return clip.clone();
    }

    let output_len = ((clip.samples.len() as u64 * u64::from(target_rate))
        / u64::from(clip.sample_rate))
        .max(1) as usize;
    let mut output = Vec::with_capacity(output_len);
    let input = &clip.samples;
    let input_len = input.len();

    for out_index in 0..output_len {
        let src_pos = (out_index as f64 * f64::from(clip.sample_rate))
            / f64::from(target_rate);
        let left = src_pos.floor() as usize;
        let right = (left + 1).min(input_len.saturating_sub(1));
        let frac = (src_pos - left as f64) as f32;
        let left_sample = f32::from(input[left.min(input_len - 1)]);
        let right_sample = f32::from(input[right]);
        let interpolated = left_sample + (right_sample - left_sample) * frac;
        output.push(interpolated.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16);
    }

    MonoPcmClip {
        sample_rate: target_rate,
        samples: output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity_when_rates_match() {
        let clip = MonoPcmClip::new(44_100, vec![100, 200, 300]);
        let resampled = resample_mono_pcm(&clip, 44_100);
        assert_eq!(resampled, clip);
    }

    #[test]
    fn resample_downscales_sample_count() {
        let clip = MonoPcmClip::new(44_100, vec![0; 44_100]);
        let resampled = resample_mono_pcm(&clip, 11_025);
        assert_eq!(resampled.sample_rate, 11_025);
        assert_eq!(resampled.samples.len(), 11_025);
    }
}
