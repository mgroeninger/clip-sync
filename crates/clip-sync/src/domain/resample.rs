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

/// Resample interleaved `i16` PCM from `from_rate` to `to_rate`.
///
/// Deinterleaves to per-channel `MonoPcmClip`, resamples each via [`resample_mono_pcm`], then
/// reinterleaves. Returns the input unchanged when rates match or when `channels` is zero.
pub fn resample_interleaved(
    samples: &[i16],
    channels: u16,
    from_rate: u32,
    to_rate: u32,
) -> Vec<i16> {
    let ch = channels as usize;
    if from_rate == to_rate || samples.is_empty() || ch == 0 {
        return samples.to_vec();
    }

    let resampled: Vec<Vec<i16>> = (0..ch)
        .map(|c| {
            let plane: Vec<i16> = samples.chunks_exact(ch).map(|frame| frame[c]).collect();
            let clip = MonoPcmClip {
                sample_rate: from_rate,
                samples: plane,
                decode_error_skips: 0,
                decoded_sample_count: None,
            };
            resample_mono_pcm(&clip, to_rate).samples
        })
        .collect();

    let out_frames = resampled.first().map_or(0, |c| c.len());
    let mut out = Vec::with_capacity(out_frames * ch);
    for f in 0..out_frames {
        for resampled_ch in &resampled {
            out.push(resampled_ch[f]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_interleaved_identity_when_rates_match() {
        let samples = vec![100_i16, -100, 200, -200];
        let out = resample_interleaved(&samples, 2, 44_100, 44_100);
        assert_eq!(out, samples);
    }

    #[test]
    fn resample_interleaved_preserves_channel_layout() {
        // Two channels: left = constant 1000, right = constant -1000.
        let rate = 44_100u32;
        let frames = rate as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            samples.push(1000_i16);
            samples.push(-1000_i16);
        }

        let out = resample_interleaved(&samples, 2, 44_100, 22_050);
        assert!(!out.is_empty());
        assert_eq!(out.len() % 2, 0, "output must have whole frames");

        let out_frames = out.len() / 2;
        let expected_frames = 22_050usize;
        assert!(
            (out_frames as i64 - expected_frames as i64).abs() <= 512,
            "out_frames={out_frames}"
        );

        // Both channels should be near their original values; no cross-channel leakage.
        let left_avg: f64 =
            out.iter().step_by(2).map(|&s| s as f64).sum::<f64>() / out_frames as f64;
        let right_avg: f64 =
            out.iter().skip(1).step_by(2).map(|&s| s as f64).sum::<f64>() / out_frames as f64;
        assert!(left_avg > 0.0, "left channel should stay positive, avg={left_avg}");
        assert!(right_avg < 0.0, "right channel should stay negative, avg={right_avg}");
    }

    #[test]
    fn resample_identity_when_rates_match() {
        let clip = MonoPcmClip {
            sample_rate: 44_100,
            samples: vec![100, 200, 300],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let resampled = resample_mono_pcm(&clip, 44_100);
        assert_eq!(resampled, clip);
    }

    #[test]
    fn resample_downscales_sample_count() {
        let clip = MonoPcmClip {
            sample_rate: 44_100,
            samples: vec![0; 44_100],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
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
