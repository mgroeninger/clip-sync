//! Synthetic timelines for energy vs bool structure acceptance (plan F1–F3).

use crate::domain::gap_energy::{build_gap_energy_signature, score_pre_energy_match, EnergyTimeline};
use crate::domain::gap_fill_fit::{
    match_gap_fill_unified_in_b, UnifiedFillMatch, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignature, GapSignatureMode};
use crate::domain::gap_structure::{score_pre_match, ActivityTimeline, StructureMatchParams};
use crate::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap, interleaved_to_channels,
    interleaved_to_mono, is_silent_frame, refine_gap_frames, FillAlignment, GapBorderSpec,
    RefinedGapFrames, SeamPlacement, SeamTemplates,
};

pub const BOOL_AMBIGUITY_EPS: f64 = 0.45;
pub const ENERGY_PAUSE_MARGIN: f64 = 0.15;
pub const MODE_SCORE_EPS: f64 = 0.08;

/// Shared geometry + PCM for one acceptance scenario.
#[derive(Debug, Clone)]
pub struct EnergySignatureFixture {
    pub id: &'static str,
    pub a_samples: Vec<f32>,
    pub b_samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
    pub gap_start: usize,
    pub gap_end: usize,
    pub context_frames: usize,
    pub true_fill_start: usize,
    pub true_fill_end: usize,
    pub nominal_fill_start: usize,
    pub nominal_fill_end: usize,
    /// F1: offset between B true dropout and the unshifted decoy copy; `0` otherwise.
    pub b_dropout_shift_frames: usize,
    pub structure_params: StructureMatchParams,
}

impl EnergySignatureFixture {
    pub fn gap_frames(&self) -> usize {
        self.gap_end.saturating_sub(self.gap_start)
    }

    pub fn bin_frames(&self) -> usize {
        self.structure_params.bin_frames
    }

    /// B-side decoy dropout start (F1 shifted duplicate); equals nominal when shift is zero.
    pub fn b_decoy_fill_start(&self) -> usize {
        if self.b_dropout_shift_frames > 0 {
            self.true_fill_start.saturating_sub(self.b_dropout_shift_frames)
        } else {
            self.nominal_fill_start
        }
    }

    pub fn signature(&self, mode: GapSignatureMode) -> GapSignature {
        build_gap_signature(
            &self.a_samples,
            self.channels,
            self.gap_start,
            self.gap_end,
            self.context_frames,
            &self.structure_params,
            mode,
        )
    }

    pub fn energy_pre_at(&self, fill_start: usize) -> f64 {
        let sig = build_gap_energy_signature(
            &self.a_samples,
            self.channels,
            self.gap_start,
            self.gap_end,
            self.context_frames,
            &self.structure_params,
        );
        let total = self.b_samples.len() / self.channels.max(1);
        let timeline = EnergyTimeline::build(
            &self.b_samples,
            self.channels,
            total,
            self.bin_frames(),
            self.structure_params.silence_peak_fraction,
            self.structure_params.absolute_silence_rms,
        );
        score_pre_energy_match(&sig, &timeline, fill_start, &self.structure_params)
    }

    pub fn energy_post_at(&self, fill_end: usize) -> f64 {
        use crate::domain::gap_energy::score_post_energy_match;
        let sig = build_gap_energy_signature(
            &self.a_samples,
            self.channels,
            self.gap_start,
            self.gap_end,
            self.context_frames,
            &self.structure_params,
        );
        let total = self.b_samples.len() / self.channels.max(1);
        let timeline = EnergyTimeline::build(
            &self.b_samples,
            self.channels,
            total,
            self.bin_frames(),
            self.structure_params.silence_peak_fraction,
            self.structure_params.absolute_silence_rms,
        );
        score_post_energy_match(&sig, &timeline, fill_end, &self.structure_params)
    }

    pub fn bool_pre_at(&self, fill_start: usize) -> f64 {
        let sig = build_gap_signature(
            &self.a_samples,
            self.channels,
            self.gap_start,
            self.gap_end,
            self.context_frames,
            &self.structure_params,
            GapSignatureMode::Bool,
        );
        let GapSignature::Bool(ref bool_sig) = sig else {
            panic!("expected bool signature");
        };
        let total = self.b_samples.len() / self.channels.max(1);
        let timeline = ActivityTimeline::build(
            &self.b_samples,
            self.channels,
            total,
            self.bin_frames(),
            self.structure_params.silence_peak_fraction,
            self.structure_params.absolute_silence_rms,
        );
        score_pre_match(bool_sig, &timeline, fill_start, &self.structure_params)
    }

    pub fn bool_post_at(&self, fill_end: usize) -> f64 {
        use crate::domain::gap_structure::score_post_match;
        let sig = build_gap_signature(
            &self.a_samples,
            self.channels,
            self.gap_start,
            self.gap_end,
            self.context_frames,
            &self.structure_params,
            GapSignatureMode::Bool,
        );
        let GapSignature::Bool(ref bool_sig) = sig else {
            panic!("expected bool signature");
        };
        let total = self.b_samples.len() / self.channels.max(1);
        let timeline = ActivityTimeline::build(
            &self.b_samples,
            self.channels,
            total,
            self.bin_frames(),
            self.structure_params.silence_peak_fraction,
            self.structure_params.absolute_silence_rms,
        );
        score_post_match(bool_sig, &timeline, fill_end, &self.structure_params)
    }

    pub fn unified_match(
        &self,
        mode: GapSignatureMode,
        weights: UnifiedFitWeights,
    ) -> Option<UnifiedFillMatch> {
        let signature = self.signature(mode);
        let ch = self.channels.max(1);
        let gap_frames = self.gap_frames();
        let border_frames = self.bin_frames() * 3;
        let border_spec = GapBorderSpec {
            gap_start_frame: self.gap_start,
            gap_end_frame: self.gap_end,
            border_frames,
            border_standoff_frames: 0,
            silence_peak_fraction: self.structure_params.silence_peak_fraction,
            absolute_rms_floor: self.structure_params.absolute_silence_rms,
        };
        let (a_pre, a_post) =
            border_templates_for_gap(&self.a_samples, ch, &border_spec);
        let (a_pre_ch, a_post_ch) =
            border_templates_per_channel_for_gap(&self.a_samples, ch, &border_spec);
        let b_mono = interleaved_to_mono(&self.b_samples, ch);
        let b_ch = interleaved_to_channels(&self.b_samples, ch);
        let pre_window = a_pre.len().max(1);
        let post_window = a_post.len().max(1);
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames,
            pre_window,
            post_window,
            b_total_frames: b_mono.len(),
            repeat_window_frames: self.bin_frames().max(1),
            repeat_penalty_weight: 0.0,
        };
        match_gap_fill_unified_in_b(
            &crate::domain::gap_fill_fit::UnifiedFillSearchInput {
                signature: &signature,
                b_samples: &self.b_samples,
                channels: self.channels,
                waveform: &waveform,
                nominal_fill_start: self.nominal_fill_start,
                nominal_fill_end: self.nominal_fill_end,
            },
            &self.structure_params,
            weights,
        )
    }

    pub fn within_bin_tolerance(&self, frame: usize, truth: usize) -> bool {
        frame.abs_diff(truth) <= self.bin_frames()
    }
}

pub fn write_frame(samples: &mut Vec<f32>, channels: usize, frame: usize, amp: f32) {
    let channels = channels.max(1);
    let needed = (frame + 1) * channels;
    if samples.len() < needed {
        samples.resize(needed, 0.0);
    }
    for ch in 0..channels {
        samples[frame * channels + ch] = amp;
    }
}

/// Overwrite `channels_to_replace` of an interleaved buffer with `gen(channel, frame)`, leaving the
/// other channels as the builder produced them. The base builders ([`write_frame`]) write identical
/// audio to every channel; this turns such a buffer into a per-channel layout — e.g. a center-dominant
/// 5.1 fixture: keep the signal channel(s) as-is and replace the surrounds/LFE with decorrelated noise
/// (different seed on A vs B → those channels do **not** cancel) or silence. Out-of-range indices are
/// ignored.
pub fn overwrite_channels(
    samples: &mut [f32],
    channels: usize,
    channels_to_replace: &[usize],
    gen: impl Fn(usize, usize) -> f32,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    for frame in 0..total_frames {
        for &ch in channels_to_replace {
            if ch < channels {
                samples[frame * channels + ch] = gen(ch, frame);
            }
        }
    }
}

/// Deterministic decorrelated noise for [`overwrite_channels`]: a per-`(seed, channel, frame)` PRNG in
/// `[-amplitude, amplitude]`. Use the **same** `seed` on A and B for a channel that should cancel
/// (same-master), a **different** `seed` for one that should not (independent capture / noise).
pub fn channel_noise(seed: u32, amplitude: f32) -> impl Fn(usize, usize) -> f32 {
    move |ch, frame| {
        let mut x = seed
            .wrapping_mul(2_654_435_761)
            .wrapping_add((ch as u32).wrapping_mul(40_503))
            .wrapping_add(frame as u32);
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let unit = (x >> 8) as f32 / (1u32 << 24) as f32; // [0, 1)
        unit * 2.0 * amplitude - amplitude
    }
}

fn write_post_with_rise(
    samples: &mut Vec<f32>,
    channels: usize,
    post_start: usize,
    post_end: usize,
    post_amp: f32,
    rise_frames: usize,
) {
    let rise_frames = rise_frames.max(1);
    for f in post_start..post_end {
        let amp = if f < post_start + rise_frames {
            let t = (f - post_start) as f32;
            t * post_amp / rise_frames as f32
        } else {
            post_amp
        };
        write_frame(samples, channels, f, amp);
    }
}

fn ramp_amp(frame: usize, ramp_end: usize, step: f32, cap: f32) -> f32 {
    if frame >= ramp_end {
        return 0.0;
    }
    (frame as f32 * step).clamp(0.0, cap)
}

struct RampGapFillSpec {
    ramp_end: usize,
    gap_start: usize,
    gap_end: usize,
    post_amp: f32,
    ramp_step: f32,
    ramp_delay: usize,
    post_rise_frames: usize,
}

fn fill_ramp_gap(
    samples: &mut Vec<f32>,
    channels: usize,
    total_frames: usize,
    spec: &RampGapFillSpec,
) {
    let RampGapFillSpec {
        ramp_end,
        gap_start,
        gap_end,
        post_amp,
        ramp_step,
        ramp_delay,
        post_rise_frames,
    } = *spec;
    for frame in 0..total_frames.min(gap_end) {
        let amp = if frame >= gap_start {
            0.0
        } else {
            ramp_amp(
                frame.saturating_sub(ramp_delay),
                ramp_end.saturating_sub(ramp_delay),
                ramp_step,
                post_amp,
            )
        };
        write_frame(samples, channels, frame, amp);
    }
    if gap_end < total_frames {
        write_post_with_rise(
            samples,
            channels,
            gap_end,
            total_frames,
            post_amp,
            post_rise_frames,
        );
    }
}

/// **F1** — ramp into gap on A; B shifted so bool pattern aliases at nominal map.
pub fn build_f1() -> EnergySignatureFixture {
    let channels = 1usize;
    let bin_frames = 10usize;
    let gap_frames = 30usize;
    let gap_start = 60usize;
    let gap_end = gap_start + gap_frames;
    let shift_frames = 15usize;
    let context_frames = 50usize;
    let total_frames = 200usize;
    let post_amp = 8_000.0_f32 / 32767.0;

    let mut a = vec![0.0f32; total_frames * channels];
    for f in 0..55 {
        let amp = ((f as f32 * 150.0).min(8_000.0)) / 32767.0;
        write_frame(&mut a, channels, f, amp);
    }
    for f in 55..gap_end {
        write_frame(&mut a, channels, f, 0.0);
    }
    write_post_with_rise(&mut a, channels, gap_end, 150, post_amp, 12);

    let mut b = vec![0.0f32; total_frames * channels];
    for f in 0..70 {
        let amp = ((f as f32 - 15.0).max(0.0) * 150.0).min(8_000.0) / 32767.0;
        write_frame(&mut b, channels, f, amp);
    }
    for f in 70..gap_end + shift_frames {
        write_frame(&mut b, channels, f, 0.0);
    }
    write_post_with_rise(
        &mut b,
        channels,
        gap_end + shift_frames,
        150,
        post_amp,
        12,
    );

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: 20,
        fill_length_slack_frames: 5,
        max_fine_adjustment_frames: 3,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F1",
        a_samples: a,
        b_samples: b,
        channels,
        sample_rate: 11_025,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start + shift_frames,
        true_fill_end: gap_end + shift_frames,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: shift_frames,
        structure_params,
    }
}

/// **F2** — two pauses in B; nominal map targets pause₂ (hard cut), truth is pause₁ (ramp).
pub fn build_f2() -> EnergySignatureFixture {
    let channels = 1usize;
    let bin_frames = 10usize;
    let gap_frames = 50usize;
    let context_frames = 50usize;
    let total_frames = 500usize;
    let pause1_start = 180usize;
    let pause1_end = pause1_start + gap_frames;
    let pause2_start = 320usize;
    let pause2_end = pause2_start + gap_frames;
    let post_amp = 8_000.0_f32 / 32767.0;
    let ramp_len = 40usize;

    let mut a = vec![0.0f32; total_frames * channels];
    for frame in 0..pause1_start {
        let amp = ramp_amp(frame, pause1_start - ramp_len, 200.0 / 32767.0, post_amp);
        write_frame(&mut a, channels, frame, amp);
    }
    write_post_with_rise(&mut a, channels, pause1_end, total_frames, post_amp, 15);

    let mut b = vec![0.0f32; total_frames * channels];
    for frame in 0..total_frames {
        let amp = if frame >= pause1_end && frame < pause2_start {
            post_amp
        } else if frame >= pause2_end {
            if frame < pause2_end + 15 {
                let t = (frame - pause2_end) as f32;
                (t * post_amp / 15.0).min(post_amp)
            } else {
                post_amp
            }
        } else if (pause1_start..pause1_end).contains(&frame)
            || (pause2_start..pause2_end).contains(&frame)
        {
            0.0
        } else {
            ramp_amp(frame, pause1_start - ramp_len, 200.0 / 32767.0, post_amp)
        };
        write_frame(&mut b, channels, frame, amp);
    }

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: 160,
        fill_length_slack_frames: 5,
        max_fine_adjustment_frames: 3,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F2",
        a_samples: a,
        b_samples: b,
        channels,
        sample_rate: 11_025,
        gap_start: pause1_start,
        gap_end: pause1_end,
        context_frames,
        true_fill_start: pause1_start,
        true_fill_end: pause1_end,
        nominal_fill_start: pause2_start,
        nominal_fill_end: pause2_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// **F3** — near-silent context (gated envelope flat); `auto` → bool.
pub fn build_f3_silence() -> EnergySignatureFixture {
    let channels = 1usize;
    let bin_frames = 10usize;
    let gap_frames = 20usize;
    let gap_start = 100usize;
    let gap_end = gap_start + gap_frames;
    let context_frames = 50usize;
    let total_frames = 200usize;

    let samples = vec![0.0f32; total_frames * channels];
    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: 10,
        fill_length_slack_frames: 2,
        max_fine_adjustment_frames: 2,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F3_silence",
        a_samples: samples.clone(),
        b_samples: samples,
        channels,
        sample_rate: 11_025,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// **F3** — steady drone; energy and bool scores agree at nominal map.
pub fn build_f3_drone() -> EnergySignatureFixture {
    let channels = 1usize;
    let bin_frames = 10usize;
    let gap_frames = 20usize;
    let gap_start = 100usize;
    let gap_end = gap_start + gap_frames;
    let context_frames = 50usize;
    let total_frames = 200usize;
    let drone = 6_000.0_f32 / 32767.0;

    let mut samples = Vec::new();
    for frame in 0..total_frames {
        let amp = if frame >= gap_start && frame < gap_end {
            0.0
        } else {
            drone
        };
        write_frame(&mut samples, channels, frame, amp);
    }

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: 10,
        fill_length_slack_frames: 2,
        max_fine_adjustment_frames: 2,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F3_drone",
        a_samples: samples.clone(),
        b_samples: samples,
        channels,
        sample_rate: 11_025,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// Weights that emphasize structure tier (acceptance tests).
pub fn structure_heavy_weights() -> UnifiedFitWeights {
    UnifiedFitWeights {
        structure_weight: 1.0,
        waveform_weight: 0.0,
        // Acceptance fixtures use a wrong nominal map on purpose; isolate structure discrimination.
        nominal_bias_scale: 0.0,
        late_start_penalty_scale: 0.0,
    }
}

/// Integration-scale **F1** at `sample_rate`.
pub fn build_f1_at_rate(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f1_scaled(sample_rate, channels)
}

pub fn build_f2_at_rate(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f2_scaled(sample_rate, channels)
}

pub fn build_f3_drone_at_rate(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f3_drone_scaled(sample_rate, channels)
}

fn frame_scale(sample_rate: u32) -> f64 {
    sample_rate as f64 / 11_025.0
}

fn scaled_usize(value: usize, scale: f64) -> usize {
    (value as f64 * scale).round() as usize
}

fn build_f1_scaled(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let scale = frame_scale(sample_rate);
    let ch = channels.max(1);
    let bin_frames = scaled_usize(10, scale).max(1);
    let gap_frames = scaled_usize(30, scale).max(1);
    let gap_start = scaled_usize(60, scale);
    let gap_end = gap_start + gap_frames;
    let shift_frames = scaled_usize(15, scale).max(1);
    let context_frames = scaled_usize(50, scale);
    let total_frames = scaled_usize(200, scale);
    let ramp_end = scaled_usize(55, scale);
    let post_amp = 8_000.0_f32 / 32767.0;

    let mut a = Vec::new();
    fill_ramp_gap(
        &mut a,
        ch,
        total_frames,
        &RampGapFillSpec {
            ramp_end,
            gap_start,
            gap_end,
            post_amp,
            ramp_step: 150.0 / 32767.0,
            ramp_delay: 0,
            post_rise_frames: scaled_usize(12, scale).max(1),
        },
    );

    let mut b = Vec::new();
    fill_ramp_gap(
        &mut b,
        ch,
        total_frames,
        &RampGapFillSpec {
            ramp_end: ramp_end + shift_frames,
            gap_start: gap_start + shift_frames,
            gap_end: gap_end + shift_frames,
            post_amp,
            ramp_step: 150.0 / 32767.0,
            ramp_delay: shift_frames,
            post_rise_frames: scaled_usize(12, scale).max(1),
        },
    );

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: scaled_usize(20, scale).max(bin_frames),
        fill_length_slack_frames: scaled_usize(5, scale).max(1),
        max_fine_adjustment_frames: scaled_usize(3, scale).max(1),
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F1",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start + shift_frames,
        true_fill_end: gap_end + shift_frames,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: shift_frames,
        structure_params,
    }
}

fn build_f2_scaled(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let scale = frame_scale(sample_rate);
    let ch = channels.max(1);
    let bin_frames = scaled_usize(10, scale).max(1);
    let gap_frames = scaled_usize(50, scale).max(1);
    let context_frames = scaled_usize(50, scale);
    let total_frames = scaled_usize(500, scale);
    let pause1_start = scaled_usize(180, scale);
    let pause1_end = pause1_start + gap_frames;
    let pause2_start = scaled_usize(320, scale);
    let pause2_end = pause2_start + gap_frames;
    let post_amp = 8_000.0_f32 / 32767.0;
    let ramp_len = scaled_usize(40, scale).max(1);

    let mut a = vec![0.0f32; total_frames * ch];
    for frame in 0..pause1_start {
        let amp = ramp_amp(frame, pause1_start - ramp_len, 200.0 / 32767.0, post_amp);
        write_frame(&mut a, ch, frame, amp);
    }
    write_post_with_rise(&mut a, ch, pause1_end, total_frames, post_amp, scaled_usize(15, scale).max(1));

    let mut b = vec![0.0f32; total_frames * ch];
    let rise = scaled_usize(15, scale).max(1);
    for frame in 0..total_frames {
        let amp = if frame >= pause1_end && frame < pause2_start {
            post_amp
        } else if frame >= pause2_end {
            if frame < pause2_end + rise {
                let t = (frame - pause2_end) as f32;
                (t * post_amp / rise as f32).min(post_amp)
            } else {
                post_amp
            }
        } else if (pause1_start..pause1_end).contains(&frame)
            || (pause2_start..pause2_end).contains(&frame)
        {
            0.0
        } else {
            ramp_amp(frame, pause1_start - ramp_len, 200.0 / 32767.0, post_amp)
        };
        write_frame(&mut b, ch, frame, amp);
    }

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: scaled_usize(160, scale).max(bin_frames),
        fill_length_slack_frames: scaled_usize(5, scale).max(1),
        max_fine_adjustment_frames: scaled_usize(3, scale).max(1),
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F2",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start: pause1_start,
        gap_end: pause1_end,
        context_frames,
        true_fill_start: pause1_start,
        true_fill_end: pause1_end,
        nominal_fill_start: pause2_start,
        nominal_fill_end: pause2_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

fn build_f3_drone_scaled(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let scale = frame_scale(sample_rate);
    let ch = channels.max(1);
    let bin_frames = scaled_usize(10, scale).max(1);
    let gap_frames = scaled_usize(20, scale).max(1);
    let gap_start = scaled_usize(100, scale);
    let gap_end = gap_start + gap_frames;
    let context_frames = scaled_usize(50, scale);
    let total_frames = scaled_usize(200, scale);
    let drone = 6_000.0_f32 / 32767.0;

    let mut samples = Vec::new();
    for frame in 0..total_frames {
        let amp = if frame >= gap_start && frame < gap_end {
            0.0
        } else {
            drone
        };
        write_frame(&mut samples, ch, frame, amp);
    }

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames,
        search_radius_frames: scaled_usize(10, scale).max(1),
        fill_length_slack_frames: scaled_usize(2, scale).max(1),
        max_fine_adjustment_frames: scaled_usize(2, scale).max(1),
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F3_drone",
        a_samples: samples.clone(),
        b_samples: samples,
        channels: ch,
        sample_rate,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

const INTEGRATION_TOTAL_SECS: f64 = 8.0;

/// Production-scale geometry for energy corpus fixtures (see `docs/archive/energy-corpus-plan.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProductionScenarioSpec {
    pub total_secs: f64,
    pub gap_signature_context_secs: f64,
    pub fill_border_search_secs: f64,
    pub fill_align_margin_secs: f64,
    pub gap_signature_bin_ms: u64,
    pub min_gap_secs: f64,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
    /// F1 context as a fraction of timeline length (integration fast path).
    pub f1_context_fraction: Option<f64>,
    /// F1 gap length as a fraction of timeline (integration fast path).
    pub f1_gap_fraction: Option<f64>,
    /// Use gap-scaled search radii from F1/F2 integration fixtures instead of `fill_border_search_secs`.
    pub integration_legacy_search: bool,
}

impl ProductionScenarioSpec {
    /// 8 s timelines matching existing I1–I4 integration tests.
    pub fn integration_fast() -> Self {
        Self {
            total_secs: INTEGRATION_TOTAL_SECS,
            gap_signature_context_secs: 2.0,
            fill_border_search_secs: 3.5,
            fill_align_margin_secs: 0.2,
            gap_signature_bin_ms: 50,
            min_gap_secs: 1.0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            f1_context_fraction: Some(0.25),
            f1_gap_fraction: Some(0.15),
            integration_legacy_search: true,
        }
    }

    /// 60 s @ production border/margin; context from caller (3 / 10 / 30 s matrix).
    pub fn production_standard(total_secs: f64, gap_signature_context_secs: f64) -> Self {
        Self {
            total_secs,
            gap_signature_context_secs,
            fill_border_search_secs: 10.0,
            fill_align_margin_secs: 1.0,
            gap_signature_bin_ms: 50,
            min_gap_secs: 1.0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            f1_context_fraction: None,
            f1_gap_fraction: None,
            integration_legacy_search: false,
        }
    }

    pub fn bin_frames(&self, sample_rate: u32) -> usize {
        ((self.gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64)
            .round()
            .max(1.0) as usize
    }

    pub fn context_frames(&self, sample_rate: u32, total_frames: usize) -> usize {
        if let Some(frac) = self.f1_context_fraction {
            (frac * total_frames as f64)
                .round()
                .max(self.bin_frames(sample_rate) as f64) as usize
        } else {
            secs_to_frames(self.gap_signature_context_secs, sample_rate)
                .max(self.bin_frames(sample_rate))
        }
    }

    pub fn search_radius_frames(&self, sample_rate: u32) -> usize {
        secs_to_frames(self.fill_border_search_secs, sample_rate)
    }

    pub fn min_gap_frames(&self, sample_rate: u32) -> usize {
        secs_to_frames(self.min_gap_secs, sample_rate)
    }

    pub fn structure_match_params(
        &self,
        sample_rate: u32,
        gap_frames: usize,
        search_radius_frames: usize,
    ) -> StructureMatchParams {
        let bin_frames = self.bin_frames(sample_rate);
        StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames,
            fill_length_slack_frames: bin_frames,
            max_fine_adjustment_frames: bin_frames,
            silence_peak_fraction: self.silence_peak_fraction,
            absolute_silence_rms: self.absolute_silence_rms,
        }
    }
}

/// Minimum timeline lead before the first gap anchor (context + border search + margin).
pub fn gap_anchor_secs(spec: &ProductionScenarioSpec) -> f64 {
    spec.gap_signature_context_secs + spec.fill_border_search_secs + spec.fill_align_margin_secs
}

/// F1 decoy shift (half gap) for production geometry checks.
pub fn f1_decoy_shift_frames(_spec: &ProductionScenarioSpec, _sample_rate: u32, gap_frames: usize) -> usize {
    gap_frames / 2
}

/// Mirror `GAP_EDGE_REFINE_SECS` in `application/patch_audio.rs`.
const PATCH_GAP_EDGE_REFINE_SECS: f64 = 0.75;

fn patch_refine_gap_frames(
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
    reported_start: usize,
    reported_end: usize,
) -> RefinedGapFrames {
    let max_refine_frames = (PATCH_GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
    refine_gap_frames(
        samples,
        channels,
        reported_start,
        reported_end,
        0.01,
        0.0,
        max_refine_frames,
    )
}

/// B-side gap bounds for oracle reports. When B still carries audio in the hole (A-only dropout),
/// silence-based peel would shrink the fill window; track A's refined hole on the shared timeline.
fn b_gap_bounds_for_report(
    fixture: &EnergySignatureFixture,
    refined_a: RefinedGapFrames,
) -> RefinedGapFrames {
    let ch = fixture.channels.max(1);
    let params = &fixture.structure_params;
    let start = fixture.nominal_fill_start;
    let end = fixture.nominal_fill_end;
    let retains_audio = !is_silent_frame(
        &fixture.b_samples,
        ch,
        start,
        params.silence_peak_fraction,
        params.absolute_silence_rms,
    ) && !is_silent_frame(
        &fixture.b_samples,
        ch,
        end.saturating_sub(1),
        params.silence_peak_fraction,
        params.absolute_silence_rms,
    );
    if retains_audio {
        refined_a
    } else {
        patch_refine_gap_frames(
            &fixture.b_samples,
            ch,
            fixture.sample_rate,
            start,
            end,
        )
    }
}

fn secs_to_frames(secs: f64, sample_rate: u32) -> usize {
    (secs * sample_rate as f64).round() as usize
}

/// **F1** geometry stretched to ~8 s for `PatchAudio` integration (I1/I2).
///
/// PCM uses a short silence lead before the reported gap; `ramp_end = silence_start` so
/// `refine_gap_frames` does not walk back into the ramp. [`gap_report_times`] applies refine
/// for patch integration.
pub fn build_f1_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f1_with_spec(sample_rate, channels, ProductionScenarioSpec::integration_fast())
}

/// **F1-long** @ 60 s with production border/context sizing (energy corpus Phase B).
pub fn build_f1_production(
    sample_rate: u32,
    channels: usize,
    gap_signature_context_secs: f64,
) -> EnergySignatureFixture {
    build_f1_production_at(sample_rate, channels, 60.0, gap_signature_context_secs)
}

/// **F1-long** at arbitrary length (e.g. 120 s for context-30 matrix / EC-5).
pub fn build_f1_production_at(
    sample_rate: u32,
    channels: usize,
    total_secs: f64,
    gap_signature_context_secs: f64,
) -> EnergySignatureFixture {
    build_f1_with_spec(
        sample_rate,
        channels,
        ProductionScenarioSpec::production_standard(total_secs, gap_signature_context_secs),
    )
}

fn build_f1_with_spec(
    sample_rate: u32,
    channels: usize,
    spec: ProductionScenarioSpec,
) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let total_frames = secs_to_frames(spec.total_secs, sample_rate);
    let bin_frames = spec.bin_frames(sample_rate);
    let gap_frames = if let Some(frac) = spec.f1_gap_fraction {
        ((frac * total_frames as f64).round() as usize).max(bin_frames * 2)
    } else {
        spec.min_gap_frames(sample_rate).max(bin_frames * 2)
    };
    let shift_frames = gap_frames / 2;
    let context_frames = spec.context_frames(sample_rate, total_frames);
    let gap_start = if spec.integration_legacy_search {
        (total_frames * 3 / 10).max(context_frames + bin_frames)
    } else {
        let anchor = secs_to_frames(gap_anchor_secs(&spec), sample_rate);
        anchor.max(context_frames + bin_frames)
    };
    let silence_lead = bin_frames;
    let silence_start = gap_start.saturating_sub(silence_lead);
    let gap_end = gap_start + gap_frames;
    let ramp_end = silence_start.saturating_sub(silence_lead);
    let post_amp = 8_000.0_f32 / 32767.0;

    let mut a = Vec::new();
    fill_ramp_gap(
        &mut a,
        ch,
        total_frames,
        &RampGapFillSpec {
            ramp_end,
            gap_start: silence_start,
            gap_end,
            post_amp,
            ramp_step: 150.0 / 32767.0,
            ramp_delay: 0,
            post_rise_frames: bin_frames.max(1),
        },
    );
    if silence_start > 0 {
        write_frame(&mut a, ch, silence_start - 1, post_amp / 2.0);
    }

    let mut b = Vec::new();
    fill_ramp_gap(
        &mut b,
        ch,
        total_frames,
        &RampGapFillSpec {
            ramp_end: ramp_end + shift_frames,
            gap_start: silence_start + shift_frames,
            gap_end: gap_end + shift_frames,
            post_amp,
            ramp_step: 150.0 / 32767.0,
            ramp_delay: shift_frames,
            post_rise_frames: bin_frames.max(1),
        },
    );
    let b_guard = silence_start + shift_frames;
    if b_guard > 0 {
        write_frame(&mut b, ch, b_guard - 1, post_amp / 2.0);
    }

    let search_radius = if spec.integration_legacy_search {
        (gap_frames * 2).max(shift_frames * 2)
    } else {
        spec.search_radius_frames(sample_rate)
    };
    let structure_params = spec.structure_match_params(
        sample_rate,
        gap_end.saturating_sub(silence_start),
        search_radius,
    );

    let id = if spec.integration_legacy_search {
        "F1_integration"
    } else {
        "F1_production"
    };

    EnergySignatureFixture {
        id,
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start: silence_start,
        gap_end,
        context_frames,
        true_fill_start: silence_start + shift_frames,
        true_fill_end: gap_end + shift_frames,
        nominal_fill_start: silence_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: shift_frames,
        structure_params,
    }
}

/// **F2** geometry stretched for integration (I3).
///
/// Unit F2 pause spacing placed ~2.35 s into an 8 s timeline (room for patch context),
/// refine guards at pause edges, scaled bins for domain search (U5 parity).
pub fn build_f2_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f2_with_spec(sample_rate, channels, ProductionScenarioSpec::integration_fast())
}

/// **F2-long** with production border/context sizing (default 90 s timeline).
pub fn build_f2_production(
    sample_rate: u32,
    channels: usize,
    total_secs: f64,
    gap_signature_context_secs: f64,
) -> EnergySignatureFixture {
    build_f2_with_spec(
        sample_rate,
        channels,
        ProductionScenarioSpec::production_standard(total_secs, gap_signature_context_secs),
    )
}

fn build_f2_with_spec(
    sample_rate: u32,
    channels: usize,
    spec: ProductionScenarioSpec,
) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let scale = frame_scale(sample_rate);
    let total_frames = secs_to_frames(spec.total_secs, sample_rate);
    let bin_frames = if spec.integration_legacy_search {
        scaled_usize(10, scale).max(1)
    } else {
        spec.bin_frames(sample_rate)
    };
    let patch_bin_frames = spec.bin_frames(sample_rate);
    let gap_frames = if spec.integration_legacy_search {
        scaled_usize(50, scale)
            .max(bin_frames * 2)
            .max(patch_bin_frames * 2)
    } else {
        spec.min_gap_frames(sample_rate)
            .max(bin_frames * 2)
            .max(patch_bin_frames * 2)
    };
    let context_frames = if spec.integration_legacy_search {
        scaled_usize(50, scale)
    } else {
        spec.context_frames(sample_rate, total_frames)
    };
    let bridge_frames = if spec.integration_legacy_search {
        gap_frames * 9 / 5
    } else {
        context_frames.max(gap_frames + bin_frames * 2)
    };
    let pause1_anchor = if spec.integration_legacy_search {
        (total_frames * 3 / 10).max(context_frames + bin_frames * 2)
    } else {
        secs_to_frames(gap_anchor_secs(&spec), sample_rate).max(context_frames + bin_frames * 2)
    };
    let gap_start = pause1_anchor;
    let gap_end = gap_start + gap_frames;
    let pause2_start = gap_end + bridge_frames;
    let pause2_end = pause2_start + gap_frames;
    let ramp_end = if spec.integration_legacy_search {
        gap_start.saturating_sub(scaled_usize(40, scale).max(1))
    } else {
        let ramp_len = spec
            .context_frames(sample_rate, total_frames)
            .max(bin_frames * 4);
        gap_start.saturating_sub(ramp_len)
    };
    let post_amp = 8_000.0_f32 / 32767.0;
    let post_rise = if spec.integration_legacy_search {
        scaled_usize(15, scale).max(1)
    } else {
        patch_bin_frames * 2
    };

    let mut a = vec![0.0f32; total_frames * ch];
    for frame in 0..gap_start {
        let amp = ramp_amp(frame, ramp_end, 200.0 / 32767.0, post_amp);
        write_frame(&mut a, ch, frame, amp);
    }
    write_post_with_rise(&mut a, ch, gap_end, total_frames, post_amp, post_rise);
    if gap_start > 0 {
        write_frame(&mut a, ch, gap_start - 1, post_amp / 2.0);
    }

    let mut b = Vec::new();
    if spec.integration_legacy_search {
        for frame in 0..total_frames {
            let amp = if frame >= gap_end && frame < pause2_start {
                post_amp
            } else if frame >= pause2_end {
                if frame < pause2_end + post_rise {
                    let t = (frame - pause2_end) as f32;
                    (t * post_amp / post_rise as f32).min(post_amp)
                } else {
                    post_amp
                }
            } else if (gap_start..gap_end).contains(&frame)
                || (pause2_start..pause2_end).contains(&frame)
            {
                0.0
            } else {
                ramp_amp(frame, ramp_end, 150.0 / 32767.0, post_amp)
            };
            write_frame(&mut b, ch, frame, amp);
        }
        if gap_start > 0 {
            write_frame(&mut b, ch, gap_start - 1, post_amp / 2.0);
        }
        if pause2_start > 0 {
            write_frame(&mut b, ch, pause2_start - 1, post_amp / 2.0);
        }
    } else {
        b = a.clone();
        for frame in pause2_start..pause2_end {
            write_frame(&mut b, ch, frame, 0.0);
        }
    }

    let search_radius = if spec.integration_legacy_search {
        scaled_usize(160, scale)
            .max(bin_frames)
            .max(pause2_start.saturating_sub(gap_start) + bin_frames)
    } else {
        spec.search_radius_frames(sample_rate)
    };
    let structure_params = if spec.integration_legacy_search {
        StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: search_radius,
            fill_length_slack_frames: scaled_usize(5, scale).max(1),
            max_fine_adjustment_frames: scaled_usize(3, scale).max(1),
            silence_peak_fraction: spec.silence_peak_fraction,
            absolute_silence_rms: spec.absolute_silence_rms,
        }
    } else {
        let mut params = spec.structure_match_params(sample_rate, gap_frames, search_radius);
        // End slack lets pause₂ bracket game post-seam scores at 50 ms bins; keep fixed gap length.
        params.fill_length_slack_frames = 0;
        params.max_fine_adjustment_frames = 0;
        params
    };

    let id = if spec.integration_legacy_search {
        "F2_integration"
    } else {
        "F2_production"
    };

    EnergySignatureFixture {
        id,
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start,
        gap_end,
        context_frames,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: pause2_start,
        nominal_fill_end: pause2_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// **F3** drone stretched for integration (I4).
pub fn build_f3_drone_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    build_f3_drone_with_total_secs(sample_rate, channels, INTEGRATION_TOTAL_SECS, None)
}

/// **F3-long** steady drone for production `auto` → bool checks.
pub fn build_f3_drone_production(
    sample_rate: u32,
    channels: usize,
    total_secs: f64,
    gap_signature_context_secs: f64,
) -> EnergySignatureFixture {
    build_f3_drone_with_total_secs(sample_rate, channels, total_secs, Some(gap_signature_context_secs))
}

fn build_f3_drone_with_total_secs(
    sample_rate: u32,
    channels: usize,
    total_secs: f64,
    gap_signature_context_secs: Option<f64>,
) -> EnergySignatureFixture {
    let spec = gap_signature_context_secs.map(|ctx| ProductionScenarioSpec::production_standard(total_secs, ctx));
    let mut fixture = build_f3_drone_scaled(sample_rate, channels);
    let ch = fixture.channels.max(1);
    let total_frames = secs_to_frames(total_secs, sample_rate);
    let current_frames = fixture.a_samples.len() / ch;
    for frame in current_frames..total_frames {
        write_frame(&mut fixture.a_samples, ch, frame, 6_000.0_f32 / 32767.0);
        write_frame(&mut fixture.b_samples, ch, frame, 6_000.0_f32 / 32767.0);
    }
    if let Some(spec) = spec {
        fixture.context_frames = spec.context_frames(sample_rate, total_frames);
        fixture.structure_params = spec.structure_match_params(
            sample_rate,
            fixture.gap_end.saturating_sub(fixture.gap_start),
            spec.search_radius_frames(sample_rate),
        );
        fixture.id = "F3_drone_production";
    } else {
        fixture.id = "F3_drone_integration";
    }
    fixture
}

/// **F4-decoy** — patch-layer `energy` vs `bool` discriminator (energy corpus Phase E.1).
///
/// Geometry mirrors F1 (A's gap sits at the **decoy** time; the true fill is shifted forward
/// in B), but the decoy and true pause carry **identical active/silent bool patterns** while
/// their **energy contours are anti-correlated**: the outer context ramps *up* into the true
/// pause (matching A) and *down* into the decoy. The immediate inner border is identical at
/// both, so the narrow waveform seam cannot tell them apart — only the wide energy envelope can.
///
/// Outcome: `bool` scores tie between decoy and truth, so `prefer_start` keeps it at the decoy
/// nominal; `energy` / `auto` resolve to the true pause. This is the EC-6 discrimination the F2
/// fixture could not provide (`b = a.clone()` made the true pause uniquely waveform-identifiable).
///
/// Context is effectively pinned to **3 s**: the decoy→truth shift is `gap + 2·context`, which
/// must stay inside `fill_border_search_secs = 10` (3 s → 7 s shift; 10 s → 21 s overruns).
pub fn build_f4_decoy_production(
    sample_rate: u32,
    channels: usize,
    total_secs: f64,
    gap_signature_context_secs: f64,
) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let spec = ProductionScenarioSpec::production_standard(total_secs, gap_signature_context_secs);
    let total_frames = secs_to_frames(total_secs, sample_rate);
    let bin = spec.bin_frames(sample_rate);
    let context = spec.context_frames(sample_rate, total_frames);
    let gap = spec.min_gap_frames(sample_rate).max(bin * 2);
    let inner = bin * 4; // identical immediate border → waveform-neutral
    let hi = 8_000.0_f32 / 32767.0;
    let lo = 3_000.0_f32 / 32767.0; // stays above the silence floor → still "active" for bool

    // Decoy sits at the A-aligned nominal; the true pause is shifted forward in B so the decoy's
    // post context and the true pause's pre context do not overlap, and the shift stays inside
    // the border search radius.
    let shift = gap + context * 2;
    debug_assert!(
        shift <= spec.search_radius_frames(sample_rate),
        "F4 decoy shift {shift} exceeds search radius (raise total/context support)"
    );
    let decoy_start = secs_to_frames(gap_anchor_secs(&spec), sample_rate).max(context + inner);
    let decoy_end = decoy_start + gap;
    let truth_start = decoy_start + shift;
    let truth_end = truth_start + gap;

    let write_ramp = |samples: &mut Vec<f32>, start: usize, end: usize, from: f32, to: f32| {
        let span = end.saturating_sub(start).max(1) as f32;
        for f in start..end {
            let t = (f - start) as f32;
            let amp = from + (to - from) * t / span;
            write_frame(samples, ch, f, amp);
        }
    };
    let write_const = |samples: &mut Vec<f32>, start: usize, end: usize, amp: f32| {
        for f in start..end {
            write_frame(samples, ch, f, amp);
        }
    };

    // Shared pause shape: ramped outer pre → steady inner border → silent gap → shared post
    // contour. `descending` flips only the outer pre ramp (decoy), keeping the inner border,
    // gap, and post identical so the bool pattern matches but the energy contour does not.
    let write_pause = |samples: &mut Vec<f32>, g_start: usize, g_end: usize, descending: bool| {
        let pre_start = g_start - context;
        let inner_start = g_start - inner;
        if descending {
            write_ramp(samples, pre_start, inner_start, hi, lo);
        } else {
            write_ramp(samples, pre_start, inner_start, lo, hi);
        }
        write_const(samples, inner_start, g_start, hi); // identical inner border
        write_const(samples, g_start, g_end, 0.0); // silent gap
        write_ramp(samples, g_end, g_end + context, hi, lo); // shared post contour
    };

    // A: content baseline with a single dropout at the decoy time (rising pre contour).
    let mut a = vec![hi; total_frames * ch];  // f32 hi
    write_pause(&mut a, decoy_start, decoy_end, false);

    // B: content baseline with the decoy (descending pre) and the true pause (rising pre).
    let mut b = vec![hi; total_frames * ch];  // f32 hi
    write_pause(&mut b, decoy_start, decoy_end, true);
    write_pause(&mut b, truth_start, truth_end, false);

    let structure_params =
        spec.structure_match_params(sample_rate, gap, spec.search_radius_frames(sample_rate));

    EnergySignatureFixture {
        id: "F4_decoy_production",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start: decoy_start,
        gap_end: decoy_end,
        context_frames: context,
        true_fill_start: truth_start,
        true_fill_end: truth_end,
        nominal_fill_start: decoy_start,
        nominal_fill_end: decoy_end,
        b_dropout_shift_frames: shift,
        structure_params,
    }
}

fn fill_speech_like(samples: &mut [f32], channels: usize, sample_rate: u32, start: usize, end: usize) {
    let ch = channels.max(1);
    for frame in start..end.min(samples.len() / ch) {
        let t = frame as f64 / sample_rate as f64;
        let amp = (0.45 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32;
        for c in 0..ch {
            samples[frame * ch + c] = amp;
        }
    }
}

fn zero_frames(samples: &mut [f32], channels: usize, start: usize, end: usize) {
    let ch = channels.max(1);
    for frame in start..end.min(samples.len() / ch) {
        for c in 0..ch {
            samples[frame * ch + c] = 0.0;
        }
    }
}

/// A1 oracle: silent dropout throat on A with salient speech bursts `peak_offset_secs` before/after.
pub fn build_speech_peaks_offset_from_throat(
    sample_rate: u32,
    channels: usize,
    peak_offset_secs: f64,
) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let spec = ProductionScenarioSpec::production_standard(60.0, 3.0);
    let total_frames = secs_to_frames(spec.total_secs, sample_rate);
    let bin_frames = spec.bin_frames(sample_rate);
    let context = spec.context_frames(sample_rate, total_frames);
    let gap_frames = spec.min_gap_frames(sample_rate).max(bin_frames * 2);
    let anchor = (gap_anchor_secs(&spec) * sample_rate as f64) as usize;
    let gap_start = anchor.max(context + bin_frames);
    let gap_end = gap_start + gap_frames;

    let peak_offset = secs_to_frames(peak_offset_secs, sample_rate);
    let burst_frames = secs_to_frames(0.35, sample_rate);
    let pre_burst_end = gap_start.saturating_sub(peak_offset);
    let pre_burst_start = pre_burst_end.saturating_sub(burst_frames);
    let post_burst_start = gap_end + peak_offset;
    let post_burst_end = (post_burst_start + burst_frames).min(total_frames);

    let mut a = vec![0.0f32; total_frames * ch];
    let mut b = vec![0.0f32; total_frames * ch];
    fill_speech_like(&mut a, ch, sample_rate, pre_burst_start, pre_burst_end);
    fill_speech_like(&mut a, ch, sample_rate, post_burst_start, post_burst_end);
    zero_frames(&mut a, ch, gap_start, gap_end);
    // B matches A outside the hole; gap region retains same-master speech for fill extraction.
    b.copy_from_slice(&a);
    fill_speech_like(&mut b, ch, sample_rate, gap_start, gap_end);

    let structure_params =
        spec.structure_match_params(sample_rate, gap_frames, spec.search_radius_frames(sample_rate));

    EnergySignatureFixture {
        id: "speech_peaks_offset_throat",
        a_samples: a,
        b_samples: b,
        channels: ch,
        sample_rate,
        gap_start,
        gap_end,
        context_frames: context,
        true_fill_start: gap_start,
        true_fill_end: gap_end,
        nominal_fill_start: gap_start,
        nominal_fill_end: gap_end,
        b_dropout_shift_frames: 0,
        structure_params,
    }
}

/// Write interleaved PCM to a WAV file.
pub fn write_pcm_wav(path: &std::path::Path, sample_rate: u32, channels: usize, samples: &[f32]) {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: channels as u16,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for &s in samples {
        let v = (s * 32767.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(v).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// A gap times plus total duration (seconds).
///
/// A times follow `refine_gap_frames` on the scan-reported [`EnergySignatureFixture::gap_start`]
/// / [`EnergySignatureFixture::gap_end`] so patch integration matches production.
pub fn gap_report_times(fixture: &EnergySignatureFixture) -> (f64, f64, f64, f64, f64) {
    let rate = fixture.sample_rate as f64;
    let ch = fixture.channels.max(1);
    let refined_a = patch_refine_gap_frames(
        &fixture.a_samples,
        ch,
        fixture.sample_rate,
        fixture.gap_start,
        fixture.gap_end,
    );
    let refined_b = b_gap_bounds_for_report(fixture, refined_a);
    let a_start = refined_a.start_frame as f64 / rate;
    let a_end = refined_a.end_frame as f64 / rate;
    let b_nominal_start = refined_b.start_frame as f64 / rate;
    let b_nominal_end = refined_b.end_frame as f64 / rate;
    let total_secs = fixture.a_samples.len() as f64 / ch as f64 / rate;
    (a_start, a_end, b_nominal_start, b_nominal_end, total_secs)
}

/// Write A/B WAV paths for an integration fixture.
pub fn write_fixture_wavs(
    dir: &std::path::Path,
    fixture: &EnergySignatureFixture,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let path_a = dir.join(format!("{}_a.wav", fixture.id));
    let path_b = dir.join(format!("{}_b.wav", fixture.id));
    write_pcm_wav(&path_a, fixture.sample_rate, fixture.channels, &fixture.a_samples);
    write_pcm_wav(&path_b, fixture.sample_rate, fixture.channels, &fixture.b_samples);
    (path_a, path_b)
}

/// Structure slide in seconds relative to nominal B map.
pub fn structure_slide_secs(fixture: &EnergySignatureFixture, start_frame: usize) -> f64 {
    let rate = fixture.sample_rate as f64;
    (start_frame as i64 - fixture.nominal_fill_start as i64) as f64 / rate
}

#[allow(dead_code)]
pub(crate) fn dummy_fill_alignment(start: usize, gap_frames: usize) -> FillAlignment {
    FillAlignment {
        start_frame: start,
        fill_frames: gap_frames,
        pre_correlation: 0.0,
        post_correlation: 0.0,
    }
}

#[allow(dead_code)]
pub(crate) fn placement_in_gap(fill_start: usize, gap_frames: usize) -> SeamPlacement {
    SeamPlacement {
        start: fill_start,
        gap_frames,
        pre_window: 8,
        post_window: 8,
    }
}

#[cfg(test)]
mod production_spec_tests {
    use super::*;

    #[test]
    fn gap_anchor_secs_sums_context_border_and_margin() {
        let spec = ProductionScenarioSpec::production_standard(60.0, 3.0);
        assert!((gap_anchor_secs(&spec) - 14.0).abs() < f64::EPSILON);
    }

    #[test]
    fn overwrite_channels_replaces_only_named_channels() {
        let channels = 3usize;
        let mut buf = vec![1.0f32; 4 * channels]; // 4 frames, all channels = 1.0
        overwrite_channels(&mut buf, channels, &[0, 2, 9], |ch, frame| (ch * 10 + frame) as f32);
        for frame in 0..4 {
            assert_eq!(buf[frame * channels], (frame) as f32, "ch0 replaced");
            assert_eq!(buf[frame * channels + 1], 1.0, "ch1 untouched");
            assert_eq!(buf[frame * channels + 2], (20 + frame) as f32, "ch2 replaced");
        }
    }

    #[test]
    fn channel_noise_is_deterministic_seed_dependent_and_bounded() {
        let a = channel_noise(7, 0.3);
        let same = channel_noise(7, 0.3);
        let diff = channel_noise(8, 0.3);
        let mut decorrelated = false;
        for frame in 0..256 {
            let v = a(1, frame);
            assert!((-0.3..0.3).contains(&v), "noise {v} within ±amplitude");
            assert_eq!(v, same(1, frame), "same seed → identical");
            if (v - diff(1, frame)).abs() > 1e-6 {
                decorrelated = true;
            }
        }
        assert!(decorrelated, "different seeds must produce different noise");
    }

    #[test]
    fn speech_peaks_fixture_unified_match_finds_truth() {
        use super::build_speech_peaks_offset_from_throat;
        use crate::domain::GapSignatureMode;
        use crate::test_support::energy_signature_fixtures::structure_heavy_weights;

        let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
        let (_, _, b0, b1, _) = gap_report_times(&fixture);
        assert!(
            (b1 - b0) > 0.5,
            "B fill window should span the nominal gap, got {b0}..{b1}"
        );
        let matched = fixture
            .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
            .expect("speech peaks fixture should align in domain");
        assert!(
            fixture.within_bin_tolerance(matched.alignment.start_frame, fixture.true_fill_start),
            "start {} vs truth {}",
            matched.alignment.start_frame,
            fixture.true_fill_start,
        );
    }

    #[test]
    fn production_anchor_places_f1_decoy_inside_border_search() {
        let spec = ProductionScenarioSpec::production_standard(60.0, 3.0);
        let rate = 48_000u32;
        let anchor = gap_anchor_secs(&spec);
        assert!(
            anchor + spec.min_gap_secs + 2.0 <= spec.total_secs,
            "gap anchor {anchor}s needs room inside {}s timeline",
            spec.total_secs
        );
        let bin = spec.bin_frames(rate);
        let gap_frames = spec.min_gap_frames(rate).max(bin * 2);
        let shift = f1_decoy_shift_frames(&spec, rate, gap_frames);
        let search = spec.search_radius_frames(rate);
        assert!(
            shift <= search,
            "decoy shift {shift} frames must be ≤ search radius {search}"
        );
    }

    #[test]
    fn integration_fast_spec_matches_legacy_total_secs() {
        let spec = ProductionScenarioSpec::integration_fast();
        assert!((spec.total_secs - INTEGRATION_TOTAL_SECS).abs() < f64::EPSILON);
        assert!(spec.integration_legacy_search);
    }

    #[test]
    fn f2_production_energy_separates_pauses() {
        use super::{build_f2_production, ENERGY_PAUSE_MARGIN};
        let f = build_f2_production(48_000, 2, 90.0, 3.0);
        let pre1 = f.energy_pre_at(f.true_fill_start);
        let pre2 = f.energy_pre_at(f.nominal_fill_start);
        let post1 = f.energy_post_at(f.true_fill_end);
        let post2 = f.energy_post_at(f.nominal_fill_end);
        assert!(
            pre1 >= pre2 + ENERGY_PAUSE_MARGIN,
            "F2-prod: energy pre should separate pauses ({pre1} vs {pre2})"
        );
        assert!(
            post1 >= post2 + ENERGY_PAUSE_MARGIN,
            "F2-prod: energy post should separate pauses ({post1} vs {post2})"
        );
    }
}
