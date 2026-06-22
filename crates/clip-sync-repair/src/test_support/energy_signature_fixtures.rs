//! Synthetic timelines for energy vs bool structure acceptance (plan F1–F3).

use crate::domain::gap_energy::{build_gap_energy_signature, score_pre_energy_match, EnergyTimeline};
use crate::domain::gap_fill_fit::{
    match_gap_fill_unified_in_b, UnifiedFillMatch, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignature, GapSignatureMode};
use crate::domain::gap_structure::{score_pre_match, ActivityTimeline, StructureMatchParams};
use crate::domain::policies::{
    border_templates_for_gap, border_templates_per_channel_for_gap, interleaved_to_channels,
    interleaved_to_mono, refine_gap_frames, FillAlignment, GapBorderSpec, RefinedGapFrames,
    SeamPlacement, SeamTemplates,
};

pub const BOOL_AMBIGUITY_EPS: f64 = 0.45;
pub const ENERGY_PAUSE_MARGIN: f64 = 0.15;
pub const MODE_SCORE_EPS: f64 = 0.08;

/// Shared geometry + PCM for one acceptance scenario.
#[derive(Debug, Clone)]
pub struct EnergySignatureFixture {
    pub id: &'static str,
    pub a_samples: Vec<i16>,
    pub b_samples: Vec<i16>,
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

pub fn write_frame(samples: &mut Vec<i16>, channels: usize, frame: usize, amp: i16) {
    let channels = channels.max(1);
    let needed = (frame + 1) * channels;
    if samples.len() < needed {
        samples.resize(needed, 0);
    }
    for ch in 0..channels {
        samples[frame * channels + ch] = amp;
    }
}

fn write_post_with_rise(
    samples: &mut Vec<i16>,
    channels: usize,
    post_start: usize,
    post_end: usize,
    post_amp: i16,
    rise_frames: usize,
) {
    let rise_frames = rise_frames.max(1);
    for f in post_start..post_end {
        let amp = if f < post_start + rise_frames {
            let t = (f - post_start) as i32;
            (t * i32::from(post_amp) / rise_frames as i32) as i16
        } else {
            post_amp
        };
        write_frame(samples, channels, f, amp);
    }
}

fn ramp_amp(frame: usize, ramp_end: usize, step: i32, cap: i16) -> i16 {
    if frame >= ramp_end {
        return 0;
    }
    ((frame as i32 * step).clamp(0, i32::from(cap))) as i16
}

struct RampGapFillSpec {
    ramp_end: usize,
    gap_start: usize,
    gap_end: usize,
    post_amp: i16,
    ramp_step: i32,
    ramp_delay: usize,
    post_rise_frames: usize,
}

fn fill_ramp_gap(
    samples: &mut Vec<i16>,
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
            0
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
    let post_amp = 8_000i16;

    let mut a = vec![0i16; total_frames * channels];
    for f in 0..55 {
        let amp = ((f as i32 * 150).min(8_000)) as i16;
        write_frame(&mut a, channels, f, amp);
    }
    for f in 55..gap_end {
        write_frame(&mut a, channels, f, 0);
    }
    write_post_with_rise(&mut a, channels, gap_end, 150, post_amp, 12);

    let mut b = vec![0i16; total_frames * channels];
    for f in 0..70 {
        let amp = ((f as i32).saturating_sub(15) * 150).clamp(0, 8_000) as i16;
        write_frame(&mut b, channels, f, amp);
    }
    for f in 70..gap_end + shift_frames {
        write_frame(&mut b, channels, f, 0);
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
    let post_amp = 8_000i16;
    let ramp_len = 40usize;

    let mut a = vec![0i16; total_frames * channels];
    for frame in 0..pause1_start {
        let amp = ramp_amp(frame, pause1_start - ramp_len, 200, post_amp);
        write_frame(&mut a, channels, frame, amp);
    }
    write_post_with_rise(&mut a, channels, pause1_end, total_frames, post_amp, 15);

    let mut b = vec![0i16; total_frames * channels];
    for frame in 0..total_frames {
        let amp = if frame >= pause1_end && frame < pause2_start {
            post_amp
        } else if frame >= pause2_end {
            if frame < pause2_end + 15 {
                let t = (frame - pause2_end) as i32;
                (t * i32::from(post_amp) / 15).min(i32::from(post_amp)) as i16
            } else {
                post_amp
            }
        } else if (pause1_start..pause1_end).contains(&frame)
            || (pause2_start..pause2_end).contains(&frame)
        {
            0
        } else {
            ramp_amp(frame, pause1_start - ramp_len, 200, post_amp)
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

    let samples = vec![0i16; total_frames * channels];
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
    let drone = 6_000i16;

    let mut samples = Vec::new();
    for frame in 0..total_frames {
        let amp = if frame >= gap_start && frame < gap_end {
            0
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
    let post_amp = 8_000i16;

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
            ramp_step: 150,
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
            ramp_step: 150,
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
    let post_amp = 8_000i16;
    let ramp_len = scaled_usize(40, scale).max(1);

    let mut a = vec![0i16; total_frames * ch];
    for frame in 0..pause1_start {
        let amp = ramp_amp(frame, pause1_start - ramp_len, 200, post_amp);
        write_frame(&mut a, ch, frame, amp);
    }
    write_post_with_rise(&mut a, ch, pause1_end, total_frames, post_amp, scaled_usize(15, scale).max(1));

    let mut b = vec![0i16; total_frames * ch];
    let rise = scaled_usize(15, scale).max(1);
    for frame in 0..total_frames {
        let amp = if frame >= pause1_end && frame < pause2_start {
            post_amp
        } else if frame >= pause2_end {
            if frame < pause2_end + rise {
                let t = (frame - pause2_end) as i32;
                (t * i32::from(post_amp) / rise as i32).min(i32::from(post_amp)) as i16
            } else {
                post_amp
            }
        } else if (pause1_start..pause1_end).contains(&frame)
            || (pause2_start..pause2_end).contains(&frame)
        {
            0
        } else {
            ramp_amp(frame, pause1_start - ramp_len, 200, post_amp)
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
    let drone = 6_000i16;

    let mut samples = Vec::new();
    for frame in 0..total_frames {
        let amp = if frame >= gap_start && frame < gap_end {
            0
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

/// Mirror `GAP_EDGE_REFINE_SECS` in `application/patch_audio.rs`.
const PATCH_GAP_EDGE_REFINE_SECS: f64 = 0.75;

fn patch_refine_gap_frames(
    samples: &[i16],
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

fn secs_to_frames(secs: f64, sample_rate: u32) -> usize {
    (secs * sample_rate as f64).round() as usize
}

/// **F1** geometry stretched to ~8 s for `PatchAudio` integration (I1/I2).
///
/// PCM uses a short silence lead before the reported gap; `ramp_end = silence_start` so
/// `refine_gap_frames` does not walk back into the ramp. [`gap_report_times`] applies refine
/// for patch integration.
pub fn build_f1_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let ch = channels.max(1);
    let total_frames = secs_to_frames(INTEGRATION_TOTAL_SECS, sample_rate);
    let bin_frames = ((0.050 * sample_rate as f64).round() as usize).max(1);
    let gap_frames = ((0.15 * total_frames as f64).round() as usize).max(bin_frames * 2);
    let shift_frames = gap_frames / 2;
    let context_frames = ((0.25 * total_frames as f64).round() as usize).max(bin_frames);
    let gap_start = (total_frames * 3 / 10).max(context_frames + bin_frames);
    let silence_lead = bin_frames;
    let silence_start = gap_start.saturating_sub(silence_lead);
    let gap_end = gap_start + gap_frames;
    let ramp_end = silence_start.saturating_sub(silence_lead);
    let post_amp = 8_000i16;

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
            ramp_step: 150,
            ramp_delay: 0,
            post_rise_frames: bin_frames.max(1),
        },
    );
    if silence_start > 0 {
        // Block refine from walking off the silence lead into the quiet ramp tail.
        write_frame(&mut a, ch, silence_start - 1, post_amp / 2);
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
            ramp_step: 150,
            ramp_delay: shift_frames,
            post_rise_frames: bin_frames.max(1),
        },
    );
    let b_guard = silence_start + shift_frames;
    if b_guard > 0 {
        write_frame(&mut b, ch, b_guard - 1, post_amp / 2);
    }

    let search_radius = (gap_frames * 2).max(shift_frames * 2);
    let structure_params = StructureMatchParams {
        gap_frames: gap_end.saturating_sub(silence_start),
        bin_frames,
        search_radius_frames: search_radius,
        fill_length_slack_frames: bin_frames,
        max_fine_adjustment_frames: bin_frames,
        silence_peak_fraction: 0.01,
        absolute_silence_rms: 0.0,
    };

    EnergySignatureFixture {
        id: "F1_integration",
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
pub fn build_f2_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let mut fixture = build_f2_scaled(sample_rate, channels);
    let ch = fixture.channels.max(1);
    let total_frames = secs_to_frames(INTEGRATION_TOTAL_SECS, sample_rate);
    let current_frames = fixture.a_samples.len() / ch;
    for frame in current_frames..total_frames {
        write_frame(&mut fixture.a_samples, ch, frame, 8_000);
        write_frame(&mut fixture.b_samples, ch, frame, 8_000);
    }
    fixture.id = "F2_integration";
    fixture
}

/// **F3** drone stretched for integration (I4).
pub fn build_f3_drone_integration(sample_rate: u32, channels: usize) -> EnergySignatureFixture {
    let mut fixture = build_f3_drone_scaled(sample_rate, channels);
    let ch = fixture.channels.max(1);
    let total_frames = secs_to_frames(INTEGRATION_TOTAL_SECS, sample_rate);
    let current_frames = fixture.a_samples.len() / ch;
    for frame in current_frames..total_frames {
        write_frame(&mut fixture.a_samples, ch, frame, 6_000);
        write_frame(&mut fixture.b_samples, ch, frame, 6_000);
    }
    fixture.id = "F3_drone_integration";
    fixture
}

/// Write interleaved PCM to a WAV file.
pub fn write_pcm_wav(path: &std::path::Path, sample_rate: u32, channels: usize, samples: &[i16]) {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: channels as u16,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for &s in samples {
        writer.write_sample(s).expect("write sample");
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
    let refined = patch_refine_gap_frames(
        &fixture.a_samples,
        ch,
        fixture.sample_rate,
        fixture.gap_start,
        fixture.gap_end,
    );
    let a_start = refined.start_frame as f64 / rate;
    let a_end = refined.end_frame as f64 / rate;
    let b_nominal_start = refined.start_frame as f64 / rate;
    let b_nominal_end = refined.end_frame as f64 / rate;
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
