use rubato::{FftFixedIn, Resampler};

use crate::domain::MonoPcmClip;

const RESAMPLE_CHUNK_SIZE: usize = 1024;
const RESAMPLE_SUB_CHUNKS: usize = 4;

/// FFT-based sinc resample mono PCM to `target_rate`. Returns the input unchanged when rates match.
pub fn resample_mono_pcm(clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip {
    if clip.sample_rate == target_rate || clip.samples.is_empty() {
        return clip.clone();
    }

    let input_rate = clip.sample_rate as usize;

    let output_rate = target_rate as usize;
    let mut resampler = match FftFixedIn::<f32>::new(
        input_rate,
        output_rate,
        RESAMPLE_CHUNK_SIZE,
        RESAMPLE_SUB_CHUNKS,
        1,
    ) {
        Ok(resampler) => resampler,
        Err(_) => return linear_resample_fallback(clip, target_rate),
    };

    let input: Vec<f32> = clip.samples.iter().map(|s| f32::from(*s)).collect();
    let mut output = Vec::new();
    let mut chunk_start = 0usize;

    while chunk_start < input.len() {
        let chunk_end = (chunk_start + RESAMPLE_CHUNK_SIZE).min(input.len());
        let chunk_len = chunk_end - chunk_start;
        let mut waves_in = vec![input[chunk_start..chunk_end].to_vec()];
        if chunk_len < RESAMPLE_CHUNK_SIZE {
            waves_in[0].resize(RESAMPLE_CHUNK_SIZE, 0.0);
        }

        let out_len = resampler.output_frames_max();
        let mut waves_out = vec![vec![0.0f32; out_len]];

        match resampler.process_into_buffer(&waves_in, &mut waves_out, None) {
            Ok((_, produced)) => output.extend_from_slice(&waves_out[0][..produced]),
            Err(_) => return linear_resample_fallback(clip, target_rate),
        }

        chunk_start += chunk_len;
    }

    let samples = output
        .into_iter()
        .map(|sample| {
            sample
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16
        })
        .collect();

    MonoPcmClip {
        sample_rate: target_rate,
        samples,
        decode_error_skips: clip.decode_error_skips,
        decoded_sample_count: clip.decoded_sample_count,
    }
}

fn linear_resample_fallback(clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip {
    let output_len = ((clip.samples.len() as u64 * u64::from(target_rate))
        / u64::from(clip.sample_rate))
        .max(1) as usize;
    let mut output = Vec::with_capacity(output_len);
    let input = &clip.samples;
    let input_len = input.len();

    for out_index in 0..output_len {
        let src_pos = (out_index as f64 * f64::from(clip.sample_rate)) / f64::from(target_rate);
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
        decode_error_skips: clip.decode_error_skips,
        decoded_sample_count: clip.decoded_sample_count,
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
        let expected = 11_025;
        assert!(
            (resampled.samples.len() as i64 - expected as i64).abs() <= 512,
            "len={}",
            resampled.samples.len()
        );
    }
}
