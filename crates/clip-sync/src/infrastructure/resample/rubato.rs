//! Rubato-backed [`Resampler`] adapter (FFT sinc resample with linear-interpolation fallback).
//!
//! Moved from `domain/resample.rs` by the layer-purity plan — the domain holds no DSP engine
//! code; `rubato` is referenced only here.

use rubato::{FftFixedIn, Resampler as _};
use tracing::warn;

use crate::application::ports::Resampler;
use crate::domain::MonoPcmClip;

const RESAMPLE_CHUNK_SIZE: usize = 1024;
const RESAMPLE_SUB_CHUNKS: usize = 4;

/// Production [`Resampler`]: rubato `FftFixedIn`, degrading to linear interpolation (with a
/// `warn!`) when the FFT engine fails to initialize or process.
pub struct RubatoResampler;

impl Resampler for RubatoResampler {
    fn resample_mono(&self, clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip {
        resample_mono_pcm(clip, target_rate)
    }
}

/// FFT-based sinc resample mono PCM to `target_rate`. Returns the input unchanged when rates match.
fn resample_mono_pcm(clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip {
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
        Err(_) => return linear_resample_fallback(clip, target_rate, "fft_init"),
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
            Err(_) => return linear_resample_fallback(clip, target_rate, "process_buffer"),
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
        // Count is denominated in source-rate samples; clear after resample so
        // decode-fraction gating cannot mix units.
        decoded_sample_count: None,
    }
}

fn linear_resample_fallback(
    clip: &MonoPcmClip,
    target_rate: u32,
    trigger: &'static str,
) -> MonoPcmClip {
    warn!(
        trigger = trigger,
        from_rate = clip.sample_rate,
        to_rate = target_rate,
        "rubato resample failed; using linear interpolation fallback"
    );

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
        decoded_sample_count: None,
    }
}

/// Resample interleaved `f32` PCM from `from_rate` to `to_rate`.
///
/// Deinterleaves to per-channel planes, resamples each via the FFT engine directly, then
/// reinterleaves. Returns the input unchanged when rates match or when `channels` is zero.
///
/// Facade convenience for `clip-sync-repair` (which calls this without port injection).
pub fn resample_interleaved(
    samples: &[f32],
    channels: u16,
    from_rate: u32,
    to_rate: u32,
) -> Vec<f32> {
    let ch = channels as usize;
    if from_rate == to_rate || samples.is_empty() || ch == 0 {
        return samples.to_vec();
    }

    let resampled: Vec<Vec<f32>> = (0..ch)
        .map(|c| {
            let plane: Vec<f32> = samples.chunks_exact(ch).map(|frame| frame[c]).collect();
            resample_f32_plane(&plane, from_rate, to_rate)
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

/// Resample a single-channel `f32` plane using `FftFixedIn`, falling back to linear interpolation.
fn resample_f32_plane(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let mut resampler = match FftFixedIn::<f32>::new(
        from_rate as usize,
        to_rate as usize,
        RESAMPLE_CHUNK_SIZE,
        RESAMPLE_SUB_CHUNKS,
        1,
    ) {
        Ok(r) => r,
        Err(_) => return linear_resample_f32(input, from_rate, to_rate, "fft_init"),
    };

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
            Err(_) => return linear_resample_f32(input, from_rate, to_rate, "process_buffer"),
        }
        chunk_start += chunk_len;
    }
    output
}

fn linear_resample_f32(
    input: &[f32],
    from_rate: u32,
    to_rate: u32,
    trigger: &'static str,
) -> Vec<f32> {
    warn!(from_rate, to_rate, trigger, "falling back to linear interleaved resample");
    let output_len =
        ((input.len() as f64 * f64::from(to_rate)) / f64::from(from_rate)).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);
    let input_len = input.len();
    for out_index in 0..output_len {
        let src_pos = (out_index as f64 * f64::from(from_rate)) / f64::from(to_rate);
        let left = src_pos.floor() as usize;
        let right = (left + 1).min(input_len.saturating_sub(1));
        let frac = (src_pos - left as f64) as f32;
        let left_sample = input[left.min(input_len - 1)];
        let right_sample = input[right];
        output.push(left_sample + (right_sample - left_sample) * frac);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_interleaved_identity_when_rates_match() {
        let samples = vec![0.003_f32, -0.003, 0.006, -0.006];
        let out = resample_interleaved(&samples, 2, 44_100, 44_100);
        assert_eq!(out, samples);
    }

    #[test]
    fn resample_interleaved_preserves_channel_layout() {
        // Two channels: left = constant 0.03, right = constant -0.03.
        let rate = 44_100u32;
        let frames = rate as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            samples.push(0.03_f32);
            samples.push(-0.03_f32);
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
    fn linear_resample_fallback_emits_warn() {
        use std::sync::{Arc, Mutex};

        use tracing::subscriber::set_default;
        use tracing_subscriber::layer::{Context, Layer};
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::Registry;

        #[derive(Default)]
        struct WarnCapture {
            messages: Arc<Mutex<Vec<String>>>,
        }

        impl<S> Layer<S> for WarnCapture
        where
            S: tracing::Subscriber,
        {
            fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
                if *event.metadata().level() != tracing::Level::WARN {
                    return;
                }
                let mut visitor = MessageVisitor::default();
                event.record(&mut visitor);
                if !visitor.message.is_empty() {
                    self.messages.lock().unwrap().push(visitor.message);
                }
            }
        }

        #[derive(Default)]
        struct MessageVisitor {
            message: String,
        }

        impl tracing::field::Visit for MessageVisitor {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "message" {
                    self.message = value.to_string();
                }
            }

            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.message = format!("{value:?}");
                }
            }
        }

        let capture = WarnCapture::default();
        let messages = Arc::clone(&capture.messages);
        let subscriber = Registry::default().with(capture);
        let _guard = set_default(subscriber);

        let clip = MonoPcmClip {
            sample_rate: 44_100,
            samples: vec![0, 100, -100, 200],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let _ = linear_resample_fallback(&clip, 11_025, "fft_init");

        assert!(
            messages
                .lock()
                .unwrap()
                .iter()
                .any(|msg| msg.contains("linear interpolation fallback")),
            "expected warn log when rubato fallback runs"
        );
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
