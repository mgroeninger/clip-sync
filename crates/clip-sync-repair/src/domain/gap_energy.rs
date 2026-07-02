//! Gated log-RMS energy envelope for structure matching.

use crate::domain::gap_structure::StructureMatchParams;
use crate::domain::policies::is_silent_frame;

const ENERGY_FLOOR: f32 = 1e-9;

/// Per-bin gated log-RMS energy (mono downmix, peak-normalized halves at match time).
pub fn energy_bins(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    bin_frames: usize,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> Vec<f32> {
    let channels = channels.max(1);
    let bin_frames = bin_frames.max(1);
    if start_frame >= end_frame {
        return Vec::new();
    }

    let mut bins = Vec::new();
    let mut frame = start_frame;
    while frame < end_frame {
        let bin_end = (frame + bin_frames).min(end_frame);
        let mut sum_sq = 0.0f64;
        let mut count = 0usize;
        for f in frame..bin_end {
            if is_silent_frame(
                samples,
                channels,
                f,
                silence_peak_fraction,
                absolute_silence_rms,
            ) {
                continue;
            }
            let mono = samples[f * channels..(f + 1) * channels]
                .iter()
                .map(|&s| s as f64)
                .sum::<f64>()
                / channels as f64;
            sum_sq += mono * mono;
            count += 1;
        }
        let rms = if count == 0 {
            0.0
        } else {
            (sum_sq / count as f64).sqrt()
        };
        let gated = if rms <= f64::from(absolute_silence_rms) {
            0.0
        } else {
            (1.0 + rms).ln() as f32
        };
        bins.push(gated);
        frame = bin_end;
    }
    bins
}

/// Pearson correlation on equal-length vectors; length mismatch → −∞.
pub fn energy_similarity(expected: &[f32], observed: &[f32]) -> f64 {
    let len = expected.len().min(observed.len());
    if len == 0 {
        return f64::NEG_INFINITY;
    }
    let expected = &expected[..len];
    let observed = &observed[..len];
    let mean_e = expected.iter().map(|v| f64::from(*v)).sum::<f64>() / len as f64;
    let mean_o = observed.iter().map(|v| f64::from(*v)).sum::<f64>() / len as f64;
    let mut num = 0.0;
    let mut den_e = 0.0;
    let mut den_o = 0.0;
    for i in 0..len {
        let e = f64::from(expected[i]) - mean_e;
        let o = f64::from(observed[i]) - mean_o;
        num += e * o;
        den_e += e * e;
        den_o += o * o;
    }
    if den_e < f64::EPSILON || den_o < f64::EPSILON {
        return f64::NEG_INFINITY;
    }
    (num / (den_e.sqrt() * den_o.sqrt())).clamp(-1.0, 1.0)
}

fn peak_normalize(values: &mut [f32]) {
    let peak = values
        .iter()
        .copied()
        .fold(0.0f32, |a, b| a.max(b))
        .max(ENERGY_FLOOR);
    for v in values {
        *v /= peak;
    }
}

/// Energy signature halves around a gap on A (same geometry as bool structure).
#[derive(Debug, Clone, PartialEq)]
pub struct GapEnergySignature {
    pub pre_energy: Vec<f32>,
    pub post_energy: Vec<f32>,
}

pub fn build_gap_energy_signature(
    samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    context_frames: usize,
    params: &StructureMatchParams,
) -> GapEnergySignature {
    let channels = channels.max(1);
    let bin_frames = params.bin_frames.max(1);
    let total_frames = samples.len() / channels;

    let pre_start = gap_start_frame.saturating_sub(context_frames);
    let pre_end = gap_start_frame.min(total_frames);
    let post_start = gap_end_frame.min(total_frames);
    let post_end = (gap_end_frame + context_frames).min(total_frames);

    let mut pre_energy = energy_bins(
        samples,
        channels,
        pre_start,
        pre_end,
        bin_frames,
        params.silence_peak_fraction,
        params.absolute_silence_rms,
    );
    let mut post_energy = energy_bins(
        samples,
        channels,
        post_start,
        post_end,
        bin_frames,
        params.silence_peak_fraction,
        params.absolute_silence_rms,
    );
    peak_normalize(&mut pre_energy);
    peak_normalize(&mut post_energy);
    GapEnergySignature {
        pre_energy,
        post_energy,
    }
}

/// Precomputed energy bins for an entire B haystack.
#[doc(hidden)]
pub struct EnergyTimeline {
    bins: Vec<f32>,
    bin_frames: usize,
}

impl EnergyTimeline {
    #[doc(hidden)]
    pub fn build(
        samples: &[f32],
        channels: usize,
        total_frames: usize,
        bin_frames: usize,
        silence_peak_fraction: f32,
        absolute_silence_rms: f32,
    ) -> Self {
        Self {
            bins: energy_bins(
                samples,
                channels,
                0,
                total_frames,
                bin_frames,
                silence_peak_fraction,
                absolute_silence_rms,
            ),
            bin_frames: bin_frames.max(1),
        }
    }

    fn bins_for_frames(&self, start_frame: usize, end_frame: usize) -> Option<&[f32]> {
        if start_frame >= end_frame {
            return None;
        }
        let start_bin = start_frame / self.bin_frames;
        let end_bin = end_frame.div_ceil(self.bin_frames);
        self.bins.get(start_bin..end_bin)
    }
}

#[doc(hidden)]
pub fn score_pre_energy_match(
    signature: &GapEnergySignature,
    timeline: &EnergyTimeline,
    fill_start: usize,
    params: &StructureMatchParams,
) -> f64 {
    let pre_span = signature.pre_energy.len() * params.bin_frames;
    if fill_start < pre_span {
        return f64::NEG_INFINITY;
    }
    let Some(b_bins) = timeline.bins_for_frames(fill_start - pre_span, fill_start) else {
        return f64::NEG_INFINITY;
    };
    let mut observed: Vec<f32> = b_bins.to_vec();
    peak_normalize(&mut observed);
    energy_similarity(&signature.pre_energy, &observed)
}

#[doc(hidden)]
pub fn score_post_energy_match(
    signature: &GapEnergySignature,
    timeline: &EnergyTimeline,
    fill_end: usize,
    params: &StructureMatchParams,
) -> f64 {
    let post_span = signature.post_energy.len() * params.bin_frames;
    let Some(b_bins) = timeline.bins_for_frames(fill_end, fill_end + post_span) else {
        return f64::NEG_INFINITY;
    };
    let mut observed: Vec<f32> = b_bins.to_vec();
    peak_normalize(&mut observed);
    energy_similarity(&signature.post_energy, &observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_structure::StructureMatchParams;

    fn write_frame(samples: &mut Vec<f32>, channels: usize, frame: usize, amp: f32) {
        while samples.len() < (frame + 1) * channels {
            samples.push(0.0);
        }
        for ch in 0..channels {
            samples[frame * channels + ch] = amp;
        }
    }

    fn params(bin_frames: usize) -> StructureMatchParams {
        StructureMatchParams {
            gap_frames: 30,
            bin_frames,
            search_radius_frames: 20,
            fill_length_slack_frames: 5,
            max_fine_adjustment_frames: 3,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        }
    }

    #[test]
    fn energy_bins_gate_silence_to_zero() {
        let mut samples = vec![0.0f32; 100];
        for f in 0..10 {
            write_frame(&mut samples, 1, f, 8_000.0 / 32767.0);
        }
        let bins = energy_bins(&samples, 1, 0, 100, 10, 0.01, 0.0);
        assert!(!bins.is_empty());
        assert!(bins[0] > 0.0);
        assert!(bins.iter().skip(1).all(|&b| b == 0.0));
    }

    #[test]
    fn energy_similarity_identical_is_one() {
        let v = vec![0.1, 0.5, 1.0, 0.3];
        let score = energy_similarity(&v, &v);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn energy_similarity_length_mismatch_is_neg_inf() {
        assert!(energy_similarity(&[1.0], &[]).is_infinite());
    }

    /// Phase 0: energy contour discriminates offset when bool active/silent pattern matches.
    #[test]
    fn energy_finds_offset_when_bool_pattern_ambiguous() {
        let channels = 1usize;
        let bin_frames = 10usize;
        let gap_frames = 30usize;
        let gap_start = 60usize;
        let gap_end = gap_start + gap_frames;
        let p = params(bin_frames);

        let mut a = vec![0.0f32; 200];
        for f in 0..55 {
            let amp = ((f as f32 * 150.0).min(8_000.0)) / 32767.0;
            write_frame(&mut a, channels, f, amp);
        }
        for f in 55..gap_end {
            write_frame(&mut a, channels, f, 0.0);
        }
        for f in gap_end..150 {
            write_frame(&mut a, channels, f, 8_000.0 / 32767.0);
        }

        let mut b_aligned = vec![0.0f32; 200];
        for f in 0..55 {
            let amp = ((f as f32 * 150.0).min(8_000.0)) / 32767.0;
            write_frame(&mut b_aligned, channels, f, amp);
        }
        for f in 55..gap_end {
            write_frame(&mut b_aligned, channels, f, 0.0);
        }
        for f in gap_end..150 {
            write_frame(&mut b_aligned, channels, f, 8_000.0 / 32767.0);
        }

        let mut b_shifted = vec![0.0f32; 200];
        for f in 0..70 {
            let amp = ((f as f32 - 15.0).max(0.0) * 150.0).min(8_000.0) / 32767.0;
            write_frame(&mut b_shifted, channels, f, amp);
        }
        for f in 70..gap_end + 15 {
            write_frame(&mut b_shifted, channels, f, 0.0);
        }
        for f in gap_end + 15..150 {
            write_frame(&mut b_shifted, channels, f, 8_000.0 / 32767.0);
        }

        let sig_a = build_gap_energy_signature(&a, channels, gap_start, gap_end, 50, &p);
        let timeline_aligned =
            EnergyTimeline::build(&b_aligned, channels, 200, bin_frames, 0.01, 0.0);
        let timeline_shifted =
            EnergyTimeline::build(&b_shifted, channels, 200, bin_frames, 0.01, 0.0);

        let aligned_pre = score_pre_energy_match(&sig_a, &timeline_aligned, gap_start, &p);
        let shifted_pre = score_pre_energy_match(&sig_a, &timeline_shifted, gap_start + 15, &p);
        assert!(
            aligned_pre > shifted_pre,
            "energy pre should prefer aligned dropout (aligned={aligned_pre}, shifted={shifted_pre})"
        );
    }
}
